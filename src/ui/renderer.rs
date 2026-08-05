// =======================================================
// src/ui/renderer.rs — wgpu graphics engine, nearest-neighbour scaling
// =======================================================

pub use crate::ui::constants::{GUI_HEIGHT, CRT_WIDTH, CRT_HEIGHT, INITIAL_WINDOW_SCALE};
use crate::ui::constants::{BUFFER_SCALE, CRT_OFFSET_X_NATIVE, CRT_OFFSET_Y_NATIVE, SHADER_SRC};
use std::sync::Arc;
use winit::window::Window;
#[cfg(not(target_os = "linux"))]
use winit::dpi::LogicalSize;

use crate::vic::constants::TOTAL_WIDTH;
#[cfg(target_os = "linux")]
use super::shell::Shell;
use super::osd::draw_status_bar;
use crate::emulator::Result;

/* Renderer owns all GPU objects needed by the main window. The emulation core supplies a completed pixel buffer; texture upload, scaling, OSD composition and presentation remain host-side concerns. */
pub struct Renderer {
	instance:          wgpu::Instance,
	adapter:           wgpu::Adapter,
	surface:           wgpu::Surface<'static>,
	device:            wgpu::Device,
	queue:             wgpu::Queue,
	surface_config:    wgpu::SurfaceConfiguration,
	pipeline:          wgpu::RenderPipeline,
	bind_group:        wgpu::BindGroup,
	bind_group_layout: wgpu::BindGroupLayout,
	texture:           wgpu::Texture,
	sampler:           wgpu::Sampler,
	pub display_buffer: Vec<u32>,
	pub osd_enabled:    bool,
	pub src_width:      usize,
	pub src_height:     usize,
	#[cfg(not(target_os = "linux"))]
	window_ref:         Arc<Window>,
}

impl Renderer {
/* Synchronous construction owns the one-time executor boundary; all GPU resource creation remains in the asynchronous helper. */
	pub fn new(window: &Arc<Window>) -> Result<Self> {
		pollster::block_on(Self::new_async(window))
	}

/* Asynchronous construction selects the adapter, configures the surface and creates resources in dependency order before any frame can be presented. */
	async fn new_async(window: &Arc<Window>) -> Result<Self> {
		let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
			backends:                 wgpu::Backends::all(),
			flags:                    wgpu::InstanceFlags::default(),
			memory_budget_thresholds: Default::default(),
			backend_options:          Default::default(),
			display:                  None,
		});

		let surface = instance.create_surface(window.clone())
			.map_err(|e| format!("Surface creation failed: {}", e))?;

		let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference:       wgpu::PowerPreference::HighPerformance,
			compatible_surface:     Some(&surface),
			force_fallback_adapter: false,
		})
		.await
		.map_err(|e| format!("No suitable wgpu adapter: {}", e))?;
		let required_limits = wgpu::Limits::downlevel_defaults()
			.using_resolution(adapter.limits());

		let (device, queue) = adapter.request_device(
			&wgpu::DeviceDescriptor {
				label:                 Some("breadbin_device"),
				required_features:     wgpu::Features::empty(),
				required_limits,
				memory_hints:          Default::default(),
				trace:                 wgpu::Trace::Off,
				experimental_features: Default::default(),
			},
		)
		.await
		.map_err(|e| format!("Device request failed: {}", e))?;

		let osd_enabled = true;
		let (src_width, src_height) = Self::get_dimensions(osd_enabled);

		let win_size     = window.inner_size();
		let surface_caps = surface.get_capabilities(&adapter);
		let surface_format = surface_caps.formats.iter()
			.find(|f| !f.is_srgb())
			.copied()
			.unwrap_or(surface_caps.formats[0]);

		let surface_config = wgpu::SurfaceConfiguration {
			usage:        wgpu::TextureUsages::RENDER_ATTACHMENT,
			format:       surface_format,
			width:        win_size.width.max(1),
			height:       win_size.height.max(1),
			present_mode: wgpu::PresentMode::AutoVsync,
			alpha_mode:   surface_caps.alpha_modes[0],
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		};
		surface.configure(&device, &surface_config);

		let (texture, sampler, bind_group_layout, bind_group, pipeline) =
			Self::build_pipeline(&device, &queue, surface_format, src_width, src_height);

		Ok(Self {
			instance,
			adapter,
			surface,
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
			#[cfg(not(target_os = "linux"))]
			window_ref:         window.clone(),
		})
	}

/* The presentation pipeline uses nearest-neighbour sampling and a fullscreen primitive so host scaling does not alter emulated pixel values. */
	pub(crate) fn build_pipeline(
		device:         &wgpu::Device,
		queue:          &wgpu::Queue,
		surface_format: wgpu::TextureFormat,
		src_width:      usize,
		src_height:     usize,
	) -> (wgpu::Texture, wgpu::Sampler, wgpu::BindGroupLayout, wgpu::BindGroup, wgpu::RenderPipeline) {
		let texture = device.create_texture(&wgpu::TextureDescriptor {
			label:           Some("screen_texture"),
			size:            wgpu::Extent3d {
				width:                 src_width as u32,
				height:                src_height as u32,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count:    1,
			dimension:       wgpu::TextureDimension::D2,
			format:          wgpu::TextureFormat::Rgba8Unorm,
			usage:           wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats:    &[],
		});

		queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture:   &texture,
				mip_level: 0,
				origin:    wgpu::Origin3d::ZERO,
				aspect:    wgpu::TextureAspect::All,
			},
			&vec![0u8; src_width * src_height * 4],
			wgpu::TexelCopyBufferLayout {
				offset:         0,
				bytes_per_row:  Some(src_width as u32 * 4),
				rows_per_image: Some(src_height as u32),
			},
			wgpu::Extent3d {
				width:                 src_width as u32,
				height:                src_height as u32,
				depth_or_array_layers: 1,
			},
		);

		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label:          Some("nearest_sampler"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			address_mode_w: wgpu::AddressMode::ClampToEdge,
			mag_filter:     wgpu::FilterMode::Nearest,
			min_filter:     wgpu::FilterMode::Nearest,
			mipmap_filter:  wgpu::MipmapFilterMode::Nearest,
			..Default::default()
		});

		let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label:   Some("bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding:    0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty:         wgpu::BindingType::Texture {
						sample_type:    wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled:   false,
					},
					count:      None,
				},
				wgpu::BindGroupLayoutEntry {
					binding:    1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count:      None,
				},
			],
		});

		let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label:   Some("screen_bg"),
			layout:  &bind_group_layout,
			entries: &[
				wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&texture_view) },
				wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
			],
		});

		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label:  Some("blit_shader"),
			source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
		});

		let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label:              Some("pipeline_layout"),
			bind_group_layouts: &[Some(&bind_group_layout)],
			immediate_size:     0,
		});

		let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label:  Some("blit_pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module:              &shader,
				entry_point:         Some("vs_main"),
				compilation_options: Default::default(),
				buffers:             &[],
			},
			fragment: Some(wgpu::FragmentState {
				module:              &shader,
				entry_point:         Some("fs_main"),
				compilation_options: Default::default(),
				targets:             &[Some(wgpu::ColorTargetState {
					format:      surface_format,
					blend:       None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			primitive:      wgpu::PrimitiveState::default(),
			depth_stencil:  None,
			multisample:    wgpu::MultisampleState::default(),
			multiview_mask: None,
			cache:          None,
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
		let (texture, sampler, bind_group_layout, bind_group, pipeline) =
			Self::build_pipeline(
				&self.device,
				&self.queue,
				self.surface_config.format,
				self.src_width,
				self.src_height,
			);
		self.texture           = texture;
		self.sampler           = sampler;
		self.bind_group_layout = bind_group_layout;
		self.bind_group        = bind_group;
		self.pipeline          = pipeline;
	}

	pub fn set_osd_enabled(&mut self, enabled: bool) {
		if self.osd_enabled == enabled { return; }
		self.osd_enabled = enabled;
		self.rebuild_dimensions();
	}

	pub fn resize_window_to_fit(&self, scale: f64) {
		let w = CRT_WIDTH as f64 * scale;
		let mut h = CRT_HEIGHT as f64 * scale;
		if self.osd_enabled {
			h += GUI_HEIGHT as f64 * scale;
		}
		#[cfg(target_os = "linux")]
		Shell::resize_content(w.round() as u32, h.round() as u32);
		#[cfg(not(target_os = "linux"))]
		let _ = self.window_ref.request_inner_size(LogicalSize::new(w, h));
	}

	/* Resizing updates only presentation geometry. The returned scale factor lets menu and window state track the effective integer display scale without changing emulated resolution. */
	pub fn handle_resize(&mut self, width: u32, height: u32) -> f64 {
		if width == 0 || height == 0 { return 0.0; }

		let ratio_w = CRT_WIDTH as f64;
		let mut ratio_h = CRT_HEIGHT as f64;
		if self.osd_enabled {
			ratio_h += GUI_HEIGHT as f64;
		}

		#[cfg(target_os = "linux")]
		let (logical_width, logical_height) = (width as f64, height as f64);
		#[cfg(not(target_os = "linux"))]
		let (logical_width, logical_height) = {
			let dpi_scale = self.window_ref.scale_factor();
			(width as f64 / dpi_scale, height as f64 / dpi_scale)
		};
		let scale = (logical_width / ratio_w).min(logical_height / ratio_h);

		self.surface_config.width  = width;
		self.surface_config.height = height;
		self.surface.configure(&self.device, &self.surface_config);

		scale
	}

	pub fn gpu_instance(&self) -> wgpu::Instance { self.instance.clone() }
	pub fn gpu_adapter(&self)  -> &wgpu::Adapter { &self.adapter }
	pub fn gpu_device(&self)   -> wgpu::Device   { self.device.clone() }
	pub fn gpu_queue(&self)    -> wgpu::Queue    { self.queue.clone() }

	/* draw uploads the latest framebuffer and optional OSD into the current swapchain image. It may recreate surface-dependent resources, but it never mutates emulated machine state. */
	pub fn draw(
		&mut self,
		vic_buffer: &[u8],
		osd: &crate::ui::osd::OsdData,
	) -> Result<()> {
		let output = match self.surface.get_current_texture() {
			wgpu::CurrentSurfaceTexture::Success(t)    => t,
			wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
			wgpu::CurrentSurfaceTexture::Timeout       => return Err("Surface timeout".into()),
			wgpu::CurrentSurfaceTexture::Occluded      => return Ok(()),
			wgpu::CurrentSurfaceTexture::Outdated      => return Err("Surface outdated".into()),
			wgpu::CurrentSurfaceTexture::Lost          => return Err("Surface lost".into()),
			wgpu::CurrentSurfaceTexture::Validation    => return Err("Surface validation error".into()),
		};

		let (src_x_off, src_y_off, src_w, src_h) =
			(CRT_OFFSET_X_NATIVE, CRT_OFFSET_Y_NATIVE, CRT_WIDTH, CRT_HEIGHT);

		let dst_w = self.src_width;
		for y_local in 0..src_h {
			let y_src = src_y_off + y_local;
			let src_row_start = y_src * TOTAL_WIDTH;
			let y_dst0 = y_local * BUFFER_SCALE;

			let row0 = y_dst0 * dst_w;
			let row1 = (y_dst0 + 1) * dst_w;

			for x_local in 0..src_w {
				let x_src = src_x_off + x_local;
				let p_idx = (src_row_start + x_src) * 3;
				let r = vic_buffer[p_idx] as u32;
				let g = vic_buffer[p_idx + 1] as u32;
				let b = vic_buffer[p_idx + 2] as u32;
				let pixel = (0xFF << 24) | (b << 16) | (g << 8) | r;

				let x_dst0 = x_local * BUFFER_SCALE;
				self.display_buffer[row0 + x_dst0]     = pixel;
				self.display_buffer[row0 + x_dst0 + 1] = pixel;
				self.display_buffer[row1 + x_dst0]     = pixel;
				self.display_buffer[row1 + x_dst0 + 1] = pixel;
			}
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
				texture:   &self.texture,
				mip_level: 0,
				origin:    wgpu::Origin3d::ZERO,
				aspect:    wgpu::TextureAspect::All,
			},
			rgba_bytes,
			wgpu::TexelCopyBufferLayout {
				offset:         0,
				bytes_per_row:  Some(self.src_width as u32 * 4),
				rows_per_image: Some(self.src_height as u32),
			},
			wgpu::Extent3d {
				width:                 self.src_width as u32,
				height:                self.src_height as u32,
				depth_or_array_layers: 1,
			},
		);

		let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

		let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("frame_encoder"),
		});

		{
			let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label:                    Some("frame_pass"),
				color_attachments:        &[Some(wgpu::RenderPassColorAttachment {
					view:           &view,
					resolve_target: None,
					depth_slice:    None,
					ops:            wgpu::Operations {
						load:  wgpu::LoadOp::Clear(wgpu::Color::BLACK),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				occlusion_query_set:      None,
				timestamp_writes:         None,
				multiview_mask:           None,
			});

			rp.set_pipeline(&self.pipeline);
			rp.set_bind_group(0, &self.bind_group, &[]);
			rp.draw(0..6, 0..1);
		}

		self.queue.submit(std::iter::once(encoder.finish()));
		output.present();

		Ok(())
	}
}