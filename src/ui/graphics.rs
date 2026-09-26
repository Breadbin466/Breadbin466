// =======================================================
// src/ui/graphics.rs — GPU resources and pixel presentation
// =======================================================

use crate::ui::constants::{BUFFER_SCALE, CRT_OFFSET_X_NATIVE, CRT_OFFSET_Y_NATIVE, SHADER_SRC};
use crate::ui::constants::{CRT_HEIGHT, CRT_WIDTH, GUI_HEIGHT};
use std::sync::Arc;
use winit::window::Window;

use super::osd::draw_status_bar;
use crate::emulator::Result;
use crate::vic::constants::TOTAL_WIDTH;

/* GPU resources are constructed on the event thread, then owned exclusively by the presentation worker. */
pub(super) struct GpuRenderer {
	surface: wgpu::Surface<'static>,
	pending_frame: Option<wgpu::SurfaceTexture>,
	device: wgpu::Device,
	queue: wgpu::Queue,
	surface_config: wgpu::SurfaceConfiguration,
	pipeline: wgpu::RenderPipeline,
	bind_group: wgpu::BindGroup,
	bind_group_layout: wgpu::BindGroupLayout,
	texture: wgpu::Texture,
	sampler: wgpu::Sampler,
	pub display_buffer: Vec<u32>,
	pub osd_enabled: bool,
	pub src_width: usize,
	pub src_height: usize,
}

impl GpuRenderer {
	/* Synchronous construction owns the one-time executor boundary; all GPU resource creation remains in the asynchronous helper. */
	pub fn new(window: &Arc<Window>) -> Result<Self> {
		pollster::block_on(Self::new_async(window))
	}

	/* Asynchronous construction selects the adapter, configures the surface and creates resources in dependency order before any frame can be presented. */
	async fn new_async(window: &Arc<Window>) -> Result<Self> {
		let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
			backends: wgpu::Backends::all(),
			flags: wgpu::InstanceFlags::default(),
			memory_budget_thresholds: Default::default(),
			backend_options: Default::default(),
			display: None,
		});

		let surface = instance
			.create_surface(window.clone())
			.map_err(|e| format!("Surface creation failed: {}", e))?;

		let adapter = instance
			.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::HighPerformance,
				compatible_surface: Some(&surface),
				force_fallback_adapter: false,
				apply_limit_buckets: false,
			})
			.await
			.map_err(|e| format!("No suitable wgpu adapter: {}", e))?;
		let required_limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());

		let (device, queue) = adapter
			.request_device(&wgpu::DeviceDescriptor {
				label: Some("breadbin_device"),
				required_features: wgpu::Features::empty(),
				required_limits,
				memory_hints: Default::default(),
				trace: wgpu::Trace::Off,
				experimental_features: Default::default(),
			})
			.await
			.map_err(|e| format!("Device request failed: {}", e))?;

		let osd_enabled = true;
		let (src_width, src_height) = Self::get_dimensions(osd_enabled);

		let win_size = window.inner_size();
		let surface_caps = surface.get_capabilities(&adapter);
		let surface_format = surface_caps
			.formats
			.iter()
			.find(|f| !f.is_srgb())
			.copied()
			.unwrap_or(surface_caps.formats[0]);

		let surface_config = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format: surface_format,
			width: win_size.width.max(1),
			height: win_size.height.max(1),
			present_mode: wgpu::PresentMode::Fifo,
			color_space: wgpu::SurfaceColorSpace::Auto,
			alpha_mode: surface_caps.alpha_modes[0],
			view_formats: vec![],
			desired_maximum_frame_latency: 1,
		};
		surface.configure(&device, &surface_config);

		let (texture, sampler, bind_group_layout, bind_group, pipeline) =
			Self::build_pipeline(&device, &queue, surface_format, src_width, src_height);

		Ok(Self {
			surface,
			pending_frame: None,
			device,
			queue,
			surface_config,
			pipeline,
			bind_group,
			bind_group_layout,
			texture,
			sampler,
			display_buffer: vec![0u32; src_width * src_height],
			osd_enabled,
			src_width,
			src_height,
		})
	}

	/* The presentation pipeline uses nearest-neighbour sampling and a fullscreen primitive so host scaling does not alter emulated pixel values. */
	pub(crate) fn build_pipeline(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		surface_format: wgpu::TextureFormat,
		src_width: usize,
		src_height: usize,
	) -> (
		wgpu::Texture,
		wgpu::Sampler,
		wgpu::BindGroupLayout,
		wgpu::BindGroup,
		wgpu::RenderPipeline,
	) {
		let texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("screen_texture"),
			size: wgpu::Extent3d {
				width: src_width as u32,
				height: src_height as u32,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8Unorm,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});

		queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &texture,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			&vec![0u8; src_width * src_height * 4],
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(src_width as u32 * 4),
				rows_per_image: Some(src_height as u32),
			},
			wgpu::Extent3d {
				width: src_width as u32,
				height: src_height as u32,
				depth_or_array_layers: 1,
			},
		);

		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("nearest_sampler"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			address_mode_w: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Nearest,
			min_filter: wgpu::FilterMode::Nearest,
			mipmap_filter: wgpu::MipmapFilterMode::Nearest,
			..Default::default()
		});

		let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
			],
		});

		let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("screen_bg"),
			layout: &bind_group_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&texture_view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&sampler),
				},
			],
		});

		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("blit_shader"),
			source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
		});

		let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("pipeline_layout"),
			bind_group_layouts: &[Some(&bind_group_layout)],
			immediate_size: 0,
		});

		let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("blit_pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: Default::default(),
				buffers: &[],
			},
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: Default::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format: surface_format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			primitive: wgpu::PrimitiveState::default(),
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			multiview_mask: None,
			cache: None,
		});

		(texture, sampler, bind_group_layout, bind_group, pipeline)
	}

	fn get_dimensions(osd_enabled: bool) -> (usize, usize) {
		let mut src_height = CRT_HEIGHT * BUFFER_SCALE;
		if osd_enabled {
			src_height += GUI_HEIGHT * BUFFER_SCALE;
		}
		(CRT_WIDTH * BUFFER_SCALE, src_height)
	}

	fn rebuild_dimensions(&mut self) {
		let (w, h) = Self::get_dimensions(self.osd_enabled);
		self.src_width = w;
		self.src_height = h;
		self.display_buffer = vec![0u32; w * h];
		self.rebuild_texture();
	}

	/* Framebuffer texture size follows the emulated image independently from the host surface size. */
	fn rebuild_texture(&mut self) {
		let (texture, sampler, bind_group_layout, bind_group, pipeline) = Self::build_pipeline(
			&self.device,
			&self.queue,
			self.surface_config.format,
			self.src_width,
			self.src_height,
		);
		self.texture = texture;
		self.sampler = sampler;
		self.bind_group_layout = bind_group_layout;
		self.bind_group = bind_group;
		self.pipeline = pipeline;
	}

	pub fn set_osd_enabled(&mut self, enabled: bool) {
		if self.osd_enabled == enabled {
			return;
		}
		self.osd_enabled = enabled;
		self.rebuild_dimensions();
	}

	pub fn handle_resize(&mut self, width: u32, height: u32) {
		self.pending_frame.take();
		self.surface_config.width = width.max(1);
		self.surface_config.height = height.max(1);
		self.surface.configure(&self.device, &self.surface_config);
	}

	/* Acquire before the worker reads the latest mailbox image. Waiting
	 * for the display cannot delay emulation or accumulate stale images. */
	pub fn prepare_frame(&mut self) -> Result<bool> {
		for _ in 0..2 {
			match self.surface.get_current_texture() {
				wgpu::CurrentSurfaceTexture::Success(output)
				| wgpu::CurrentSurfaceTexture::Suboptimal(output) => {
					self.pending_frame = Some(output);
					return Ok(true);
				}
				wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => return Ok(false),
				/* A display transition may invalidate the surface without losing the device. Reconfigure and retry once, never spin on an unavailable surface. */
				wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => self.surface.configure(&self.device, &self.surface_config),
				wgpu::CurrentSurfaceTexture::Validation => return Err("Surface validation error".into()),
			}
		}
		Ok(false)
	}
	/* Upload and present the latest completed framebuffer without advancing
	 * hardware. Repeated host refreshes retain the same sharp PAL image. */
	pub fn draw(&mut self, vic_buffer: &[u8], osd: &crate::ui::osd::OsdData) -> Result<()> {
		let Some(output) = self.pending_frame.take() else {
			return Err("No acquired presentation image".into());
		};

		/* Convert one native row, then duplicate it in one contiguous copy.
		 * Chunked slices keep bounds checks out of the per-pixel expansion. */
		let dst_w = self.src_width;
		for (y, rows) in self.display_buffer[..CRT_HEIGHT * BUFFER_SCALE * dst_w]
			.chunks_exact_mut(dst_w * BUFFER_SCALE).enumerate()
		{
			let start = ((CRT_OFFSET_Y_NATIVE + y) * TOTAL_WIDTH + CRT_OFFSET_X_NATIVE) * 3;
			let source = &vic_buffer[start..start + CRT_WIDTH * 3];
			let (first, second) = rows.split_at_mut(dst_w);
			for (rgb, pair) in source.chunks_exact(3).zip(first.chunks_exact_mut(BUFFER_SCALE)) {
				let pixel = u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 0xFF]);
				pair.fill(pixel);
			}
			second.copy_from_slice(first);
		}

		if self.osd_enabled {
			draw_status_bar(
				&mut self.display_buffer,
				self.src_width,
				self.src_height,
				GUI_HEIGHT * BUFFER_SCALE,
				osd,
			);
		}

		let rgba_bytes: &[u8] = bytemuck::cast_slice(&self.display_buffer);

		self.queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &self.texture,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			rgba_bytes,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(self.src_width as u32 * 4),
				rows_per_image: Some(self.src_height as u32),
			},
			wgpu::Extent3d {
				width: self.src_width as u32,
				height: self.src_height as u32,
				depth_or_array_layers: 1,
			},
		);

		let view = output
			.texture
			.create_view(&wgpu::TextureViewDescriptor::default());

		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("frame_encoder"),
			});

		{
			let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("frame_pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					resolve_target: None,
					depth_slice: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				occlusion_query_set: None,
				timestamp_writes: None,
				multiview_mask: None,
			});

			rp.set_pipeline(&self.pipeline);
			rp.set_bind_group(0, &self.bind_group, &[]);
			rp.draw(0..6, 0..1);
		}

		self.queue.submit(std::iter::once(encoder.finish()));
		self.queue.present(output);

		Ok(())
	}
}
impl Drop for GpuRenderer {
	fn drop(&mut self) {
		self.pending_frame.take();
	}
}