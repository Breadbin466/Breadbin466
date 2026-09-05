// =======================================================
// src/ui/inspector.rs — Diagnostic "Inspector" window: live state readout
// =======================================================

use crate::ui::constants::{INSPECTOR_HEIGHT, INSPECTOR_WIDTH, LINE_HEIGHT, MARGIN_X, MARGIN_Y};
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use super::osd::draw_string;
use crate::emulator::Result;

/* InspectorWindow is a fixed-size satellite renderer. It shares the main wgpu instance, adapter, device and queue, but owns an independent window surface, texture and text buffer. */
pub struct InspectorWindow {
	pub window: Arc<Window>,
	surface: wgpu::Surface<'static>,
	device: wgpu::Device,
	queue: wgpu::Queue,
	surface_config: wgpu::SurfaceConfiguration,
	pipeline: wgpu::RenderPipeline,
	bind_group: wgpu::BindGroup,
	texture: wgpu::Texture,
	buffer: Vec<u32>,
}

impl InspectorWindow {
	/* Construction creates only per-window GPU resources; sharing the device avoids a second adapter selection and keeps diagnostic rendering in the same graphics context as the main display. */
	pub fn new(
		application: &ActiveEventLoop,
		instance: wgpu::Instance,
		adapter: &wgpu::Adapter,
		device: wgpu::Device,
		queue: wgpu::Queue,
		format: wgpu::TextureFormat,
	) -> Result<Self> {
		let window_attrs = Window::default_attributes()
			.with_title("Inspector")
			.with_inner_size(LogicalSize::new(
				INSPECTOR_WIDTH as f64,
				INSPECTOR_HEIGHT as f64,
			))
			.with_resizable(false);

		let window = Arc::new(application.create_window(window_attrs)?);

		let surface = instance
			.create_surface(window.clone())
			.map_err(|e| format!("Inspector surface creation failed: {}", e))?;

		let caps = surface.get_capabilities(adapter);
		let surface_format = if caps.formats.contains(&format) {
			format
		} else {
			caps.formats.first().copied().unwrap_or(format)
		};

		let win_size = window.inner_size();
		let surface_config = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format: surface_format,
			width: win_size.width.max(1),
			height: win_size.height.max(1),
			present_mode: wgpu::PresentMode::AutoVsync,
			alpha_mode: caps.alpha_modes[0],
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		};
		surface.configure(&device, &surface_config);

		let (texture, _sampler, _bgl, bind_group, pipeline) =
			super::renderer::Renderer::build_pipeline(
				&device,
				&queue,
				surface_format,
				INSPECTOR_WIDTH,
				INSPECTOR_HEIGHT,
			);

		Ok(Self {
			window,
			surface,
			device,
			queue,
			surface_config,
			pipeline,
			bind_group,
			texture,
			buffer: vec![0xFFFFFFFF; INSPECTOR_WIDTH * INSPECTOR_HEIGHT],
		})
	}

	pub fn id(&self) -> WindowId {
		self.window.id()
	}

	pub fn request_redraw(&self) {
		self.window.request_redraw();
	}

	/* Drawing rasterises diagnostic text into a CPU buffer, uploads that texture, and presents it through the inspector surface without touching emulated state. */
	pub fn draw(&mut self, lines: &[String], is_dark_mode: bool) -> Result<()> {
		let color_bg = if is_dark_mode { 0xFF000000 } else { 0xFFFFFFFF };
		let color_text = if is_dark_mode { 0xFFFFFFFF } else { 0xFF000000 };
		let clear_color = if is_dark_mode {
			wgpu::Color::BLACK
		} else {
			wgpu::Color::WHITE
		};

		self.buffer.fill(color_bg);

		let mut y = MARGIN_Y;
		for line in lines {
			if y + 8 > INSPECTOR_HEIGHT {
				break;
			}
			draw_string(
				&mut self.buffer,
				INSPECTOR_WIDTH,
				INSPECTOR_HEIGHT,
				MARGIN_X,
				y,
				line,
				color_text,
			);
			y += LINE_HEIGHT;
		}

		let output = match self.surface.get_current_texture() {
			wgpu::CurrentSurfaceTexture::Success(t) => t,
			wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
			wgpu::CurrentSurfaceTexture::Timeout => return Err("Inspector surface timeout".into()),
			wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
			wgpu::CurrentSurfaceTexture::Outdated => {
				return Err("Inspector surface outdated".into());
			}
			wgpu::CurrentSurfaceTexture::Lost => return Err("Inspector surface lost".into()),
			wgpu::CurrentSurfaceTexture::Validation => {
				return Err("Inspector surface validation error".into());
			}
		};

		let rgba_bytes: &[u8] = bytemuck::cast_slice(&self.buffer);
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
				bytes_per_row: Some(INSPECTOR_WIDTH as u32 * 4),
				rows_per_image: Some(INSPECTOR_HEIGHT as u32),
			},
			wgpu::Extent3d {
				width: INSPECTOR_WIDTH as u32,
				height: INSPECTOR_HEIGHT as u32,
				depth_or_array_layers: 1,
			},
		);

		let view = output
			.texture
			.create_view(&wgpu::TextureViewDescriptor::default());
		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("inspector_encoder"),
			});
		{
			let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("inspector_pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					resolve_target: None,
					depth_slice: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(clear_color),
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
		output.present();

		Ok(())
	}

	pub fn handle_resize(&mut self, width: u32, height: u32) {
		if width == 0 || height == 0 {
			return;
		}
		self.surface_config.width = width;
		self.surface_config.height = height;
		self.surface.configure(&self.device, &self.surface_config);
	}
}