// =======================================================
// src/vic/sequencer.rs — VIC-II cycle sequencer and rendering orchestration
// =======================================================

use super::fsm::GraphicsMode;
use crate::memory::Memory;
use super::{fsm::{CharacterAccessState, DisplayState, DisplayTransition}, state::VicII};
use super::bus_access::{get_vic_bank, vic_read};
use super::constants::{IRQ_RASTER, IDLE_ACCESS_ADDRESS, HPIXEL_START, PAL_LAST_CYCLE, SPRITE_DISPLAY_CYCLE, POST_SPRITE_CYCLE};
use super::{foreground::{ForegroundCell, PixelSpan}, foreground_renderer::render_foreground_cell, foreground_transition::render_foreground_span};
use super::border_renderer::{draw_border, draw_border4, draw_border7, render_38_right, render_38_left, render_40_left_first, render_40_left_second};
use super::bus_access::floating_bus_read;
/*
The sequencer is the VIC-II's per-cycle schedule. Each call first advances raster timing and latches live control bits, then performs display and border work for the previous fetch pipeline, and finally executes the memory slot belonging to the new raster cycle. Sprite DMA, DRAM refresh, character accesses and graphics accesses therefore compete through one ordered schedule instead of independent subsystems. The cycle numbers follow Bauer's PAL convention, starting at 1. (BAUER-VIC-II-1996, sections 3.5 to 3.9)
*/
impl VicII {
	#[inline(always)]
	/*
	One VIC cycle advances both the externally visible raster position and the internal fetch/render pipelines. Rendering is deliberately interleaved with bus activity because border openings, mode changes and sprite priority can become visible within the same eight-pixel cycle.
	*/
	pub fn tick_sequencer(&mut self, memory: &mut Memory, master_cycle: u64) {
		self.current_clock += 2;
		self.sprites.current_cycle_counter = self.current_clock;
		self.sprites.tick_collision_clear();
		let ctrl1 = self.regs.ctrl1;
		let ctrl2 = self.regs.ctrl2;
		let den = (ctrl1 & 0x10) != 0;
		let rsel = (ctrl1 & 0x08) != 0;
		let y_scroll = ctrl1 & 0x07;
		self.timing.latch_registers(den, rsel, y_scroll);
		self.timing.tick();
		if self.timing.cycle != 1 {
			let raster_line = self.timing.raster_line;
			let latch_den = self.latch_den;
			if self.timing.update_badline_live(raster_line, y_scroll, latch_den) {
				self.display_state = DisplayState::Display;
			}
		}
		let cycle = self.timing.cycle;
		let cycle_prev: u8 = if cycle == 1 { PAL_LAST_CYCLE as u8 } else { (cycle - 1) as u8 };
		let bank = get_vic_bank(memory);
		if cycle == 1 {
			self.begin_frame_line(memory, bank, master_cycle, cycle_prev);
		}
		/* Cycle 2 completes the frame wrap prepared at cycle 1, resets frame-scoped latches and performs the first sprite data slot of the new line. */
		if cycle == 2 {
			self.draw_active_sprites(cycle_prev);
			if self.check_irq_in_cycle2 {
				self.check_irq_in_cycle2 = false;
				self.timing.reset_line_to_zero();
				self.dram_refresh_counter = 0xFF;
				self.rearm_light_pen();
				self.latch_den = false;
				self.vc_base = 0;
				if self.regs.raster_irq == 0 {
					self.irq.trigger(IRQ_RASTER);
				}
			}
			self.update_bus_arbitration(cycle);
			self.sprite_first_and_second_data_access(3, memory, bank, master_cycle);
		}
		let line = self.timing.raster_line;
		/* Cycles 3 through 8 finish the sprite pixels whose shifters were armed on the preceding line; their data fetches occur later in the current line. */
		if cycle >= 3 && cycle <= 8 {
			self.draw_active_sprites(cycle_prev);
		}
		self.sprite_ptr_data_slots(memory, bank, master_cycle, cycle);
		/* Mode selection is sampled for rendering every cycle so register writes can alter the interpretation of data already resident in the graphics pipeline. */
		let graphics_mode = GraphicsMode::from_control_bits((ctrl1 & 0x40) != 0, (ctrl1 & 0x20) != 0, (ctrl2 & 0x10) != 0);
		self.graphics_mode = graphics_mode;
		let csel = (ctrl2 & 0x08) != 0;
		let render_live = self.screen.compose_video() || self.sprites.any_armed_or_active();
		/* Rendering may still be required when normal video output is suppressed, because armed sprites continue to generate collisions and priority masks. */
		if render_live {
			let horizontal_scroll = self.regs.x_scroll() as i8;
			match cycle {
			9..=16 => {
				if render_live {
					let cell = ForegroundCell::blank(horizontal_scroll as u8, self.char_data_fetched, graphics_mode, cycle);
					render_foreground_cell(&mut self.screen, cell, &mut self.pixels_to_skip);
					self.draw_active_sprites(cycle_prev);
					if cycle >= 10 {
						draw_border(&mut self.screen, &self.border, cycle);
						self.screen.color_foreground(cycle);
					}
				}
			}
			17 => {
				self.border.update_left_border_40_column(csel, line, den);
				if render_live {
					self.draw_visible_foreground(horizontal_scroll, cycle);
					self.draw_active_sprites(cycle_prev);
					draw_border4(&mut self.screen, &self.border, cycle);
					render_40_left_first(&mut self.screen, &self.border, cycle);
					self.screen.color_foreground(cycle);
				}
			}
			18 => {
				if render_live {
					self.draw_visible_foreground(horizontal_scroll, cycle);
					self.draw_active_sprites(cycle_prev);
				}
				self.border.update_left_border_38_column(csel, line, den);
				if render_live {
					render_40_left_second(&mut self.screen, &self.border, cycle);
					render_38_left(&mut self.screen, &self.border, cycle);
					self.screen.color_foreground(cycle);
				}
			}
			19..=54 => {
				if render_live {
					self.draw_visible_foreground(horizontal_scroll, cycle);
					self.draw_active_sprites(cycle_prev);
					draw_border(&mut self.screen, &self.border, cycle);
					self.screen.color_foreground(cycle);
				}
			}
			55 => {
				if render_live {
					self.draw_visible_foreground(horizontal_scroll, cycle);
					self.draw_active_sprites(cycle_prev);
					draw_border(&mut self.screen, &self.border, cycle);
					self.screen.color_foreground(cycle);
				}
				self.character_access_state = CharacterAccessState::Disabled;
				self.sprites.start_sprite_dma_first_phase(line, &self.regs);
			}
			/* After the final graphics access, cycle 56 performs an idle read but still advances the graphics pipeline so its last cell can be rendered. */
			56 => {
				if render_live {
					self.draw_visible_foreground(horizontal_scroll, cycle);
					if self.border.char_data_output_disabled {
						self.screen.mask_buf[cycle as usize] = 0;
					}
					self.draw_active_sprites(cycle_prev);
					draw_border7(&mut self.screen, &self.border, cycle);
				}
				self.border.update_right_border_38_column(csel);
				if render_live {
					render_38_right(&mut self.screen, &self.border, cycle);
					self.screen.color_foreground(cycle);
				}
				self.sprites.start_sprite_dma_second_phase(line, &self.regs);
				self.horizontal_scroll_at_cycle57 = ctrl2 & 0x07;
			}
			/* Cycle 57 drains the remaining pipeline stages and clears the character input latch before sprite pointer/data slots take over the bus. */
			57 => {
				if render_live {
					let count = (8i16 - self.horizontal_scroll_at_cycle57 as i16 + (ctrl2 & 0x07) as i16) as u8;
					let cell = ForegroundCell::new(0, self.horizontal_scroll_at_cycle57 as i8, self.char_data_fetched, graphics_mode, 0, self.border.char_data_output_disabled, cycle);
					render_foreground_span(&mut self.screen, cell, PixelSpan::new(0, count), &mut self.pixels_to_skip);
					self.draw_active_sprites(cycle_prev);
					draw_border(&mut self.screen, &self.border, cycle);
				}
				self.border.update_right_border_40_column(csel);
				if render_live {
					self.screen.color_foreground(cycle);
				}
			}
			SPRITE_DISPLAY_CYCLE..=PAL_LAST_CYCLE => {
				if render_live {
					let cell = ForegroundCell::blank(horizontal_scroll as u8, self.char_data_fetched, graphics_mode, cycle);
					render_foreground_cell(&mut self.screen, cell, &mut self.pixels_to_skip);
					self.draw_active_sprites(cycle_prev);
					draw_border(&mut self.screen, &self.border, cycle);
					self.screen.color_foreground(cycle);
				}
			}
				_ => {}
			}
		} else {
			match cycle {
				17 => self.border.update_left_border_40_column(csel, line, den),
				18 => self.border.update_left_border_38_column(csel, line, den),
				55 => {
					self.character_access_state = CharacterAccessState::Disabled;
					self.sprites.start_sprite_dma_first_phase(line, &self.regs);
				}
				56 => {
					self.border.update_right_border_38_column(csel);
					self.sprites.start_sprite_dma_second_phase(line, &self.regs);
					self.horizontal_scroll_at_cycle57 = ctrl2 & 0x07;
				}
				57 => self.border.update_right_border_40_column(csel),
				_ => {}
			}
		}
		/* The video-matrix base is re-derived at the bus phase boundary, allowing a $D018 write to affect subsequent character accesses without rewriting earlier fetches. */
		let vm_base = ((self.regs.mem_ptrs as u16) & 0xF0) << 6;
		/*
		The second half of the schedule performs memory traffic. Cycles 11 to 14 refresh DRAM, cycles 15 to 54 form the forty character/graphics fetch pairs, and the end of the line is reserved for idle accesses and sprite DMA.
		*/
		match cycle {
			3..=10 => {
				self.update_bus_arbitration(cycle);
			}
			11..=13 => {
				self.update_bus_arbitration(cycle);
				self.dram_refresh(memory, bank, master_cycle);
			}
			/* Cycle 14 closes refresh, reloads VC from VCBASE and prepares RC for a badline before the first matrix access. */
			14 => {
				self.update_bus_arbitration(cycle);
				self.dram_refresh(memory, bank, master_cycle);
				self.vc = self.vc_base;
				self.vmli = 0;
				if self.timing.is_badline {
					self.rc = 0;
				}
			}
			/* Cycle 15 is the first character access. Character fetches remain enabled for the forty-column window only when the line is a badline. */
			15 => {
				self.update_bus_arbitration(cycle);
				self.dram_refresh(memory, bank, master_cycle);
				self.character_access_state = if self.timing.is_badline { CharacterAccessState::Enabled } else { CharacterAccessState::Disabled };
				self.display_transition = DisplayTransition::Stable;
				self.character_access(memory, bank, master_cycle, vm_base);
			}
			/* Cycle 16 begins the steady c-access/g-access pipeline: old data shifts towards rendering while fresh graphics and matrix data enter stage one. */
			16 => {
				self.sprites.update_mcbase_and_dma();
				self.update_bus_arbitration(cycle);
				self.border.latch_main_border_old();
				self.shift_graphics_pipelines();
				self.graphics_data_fetched = self.graphics_access(memory, bank, master_cycle);
				self.character_access(memory, bank, master_cycle, vm_base);
			}
			17 => {
				self.update_bus_arbitration(cycle);
				self.border.latch_main_border_old();
				self.shift_graphics_pipelines();
				self.graphics_data_fetched = self.graphics_access(memory, bank, master_cycle);
				self.character_access(memory, bank, master_cycle, vm_base);
			}
			18..=54 => {
				self.update_bus_arbitration(cycle);
				self.shift_graphics_pipelines();
				self.graphics_data_fetched = self.graphics_access(memory, bank, master_cycle);
				self.character_access(memory, bank, master_cycle, vm_base);
			}
			55 => {
				self.update_bus_arbitration(cycle);
				self.shift_graphics_pipelines();
				self.graphics_data_fetched = self.graphics_access(memory, bank, master_cycle);
			}
			56 => {
				self.update_bus_arbitration(cycle);
				vic_read(memory, IDLE_ACCESS_ADDRESS, bank, master_cycle);
				self.shift_graphics_pipelines();
				self.graphics_data_fetched = 0;
			}
			57 => {
				self.update_bus_arbitration(cycle);
				vic_read(memory, IDLE_ACCESS_ADDRESS, bank, master_cycle);
				self.char_data_pipeline_2 = self.char_data_pipeline_1;
				self.graphics_data_pipeline_2 = self.graphics_data_pipeline_1;
				self.char_data_pipeline_1 = 0;
			}
			/* Cycle 58 transfers fetched sprite data into the display engines, performs sprite 0 pointer/data traffic and advances RC/VCBASE at the character-row boundary. */
			58 => {
				self.graphics_data_pipeline_2 = 0;
				self.update_bus_arbitration(cycle);
				self.sprites.load_mc_and_update_sprite_display(line, &self.regs);
				self.sprites.slot_ptr(0, memory, vm_base, bank, master_cycle);
				if (self.sprites.sprite_dma & 1) != 0 {
					self.sprites.slot_data(0, 2, memory, bank, master_cycle, self.aec_low, true);
				} else {
					let value = floating_bus_read(memory, bank, master_cycle, self.aec_low);
					self.sprites.sprites[0].fetch_buf[2] = value;
				}
				if self.rc == 7 {
					self.display_state = DisplayState::Idle;
					self.vc_base = self.vc;
				}
				if !self.display_state.is_idle() || self.timing.is_badline {
					self.rc = (self.rc + 1) & 7;
					self.display_state = DisplayState::Display;
				}
			}
			POST_SPRITE_CYCLE..=PAL_LAST_CYCLE => {
				self.update_bus_arbitration(cycle);
			}
			_ => {}
		}
		if cycle == PAL_LAST_CYCLE {
			self.border.tick_cycle63(line, self.regs.den());
		}
		if self.mode_changing {
			self.mode_changing = false;
			self.previous_graphics_mode = self.graphics_mode;
		}
		self.horizontal_pixel_counter = HPIXEL_START;
		self.irq_changed = self.irq.line_active;
	}
}