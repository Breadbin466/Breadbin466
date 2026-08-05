// =======================================================
// src/vic/border_renderer.rs — Border pixel output: full-width, partial and 38/40-column left/right edge draws
// =======================================================

use crate::vic::screen::Screen;
use crate::vic::constants::BORDERCOLORINDEX;
use crate::vic::border::BorderUnit;

/*
Border rendering consumes the flip-flop history recorded by BorderUnit. Several entry points draw fewer than eight pixels because the horizontal comparators can change the border state within a character cell; preserving those partial spans is what makes side-border opening effects visible. (BAUER-VIC-II-1996, border generation)
*/

/*
Raster cycles are translated into the line-buffer coordinate system whose visible origin is offset from cycle zero. Negative positions belong to the non-buffered portion of the PAL line and are therefore discarded.
*/
#[inline(always)]
fn span_base(cycle: u16, offset: isize) -> Option<usize> {
	let base = cycle as isize * 8 - 20 + offset;
	(base >= 0).then_some(base as usize)
}

/*
A border span updates both the colour buffer and the independent border mask. The mask is retained until final composition so sprites cannot incorrectly appear in front of a border pixel.
*/
#[inline(always)]
fn fill_border_span(screen: &mut Screen, base: usize, count: usize) {
	if !screen.compose_video() { return; }
	let Some(target) = screen.line_buf.get_mut(base..base + count) else { return; };
	target.fill(BORDERCOLORINDEX);
	screen.set_border_span(base, count);
}

#[inline(always)]
/* Draw the full eight-pixel cell when the main border flip-flop is already set for the whole slot. */
pub fn draw_border(screen: &mut Screen, border: &BorderUnit, cycle: u16) {
	if !border.main_border { return; }
	let Some(base) = span_base(cycle, 0) else { return; };
	fill_border_span(screen, base, 8);
}

#[inline(always)]
/* Draw the first four pixels from the border state latched before a mid-cell comparator change. */
pub fn draw_border4(screen: &mut Screen, border: &BorderUnit, cycle: u16) {
	if !border.main_border_old { return; }
	let Some(base) = span_base(cycle, 0) else { return; };
	fill_border_span(screen, base, 4);
}

#[inline(always)]
/* Draw the seven-pixel body used by the 38-column right-edge split before its final comparator pixel. */
pub fn draw_border7(screen: &mut Screen, border: &BorderUnit, cycle: u16) {
	if !border.main_border { return; }
	let Some(base) = span_base(cycle, 0) else { return; };
	fill_border_span(screen, base, 7);
}

#[inline(always)]
/* Draw the final single pixel selected by the 38-column right comparator. */
pub fn render_38_right(screen: &mut Screen, border: &BorderUnit, cycle: u16) {
	if !border.main_border { return; }
	let Some(pos) = span_base(cycle, 7) else { return; };
	fill_border_span(screen, pos, 1);
}

/*
In 38-column mode the left comparator can change during the eight-pixel output slot. border_part_38 records the seven-pixel body and final single pixel separately so a write to CSEL can open or close only the portion that has not yet passed the comparator.
*/
#[inline(always)]
pub fn render_38_left(screen: &mut Screen, border: &BorderUnit, cycle: u16) {
	let Some(base) = span_base(cycle, 0) else { return; };
	if (border.border_part_38 & 1) != 0 {
		fill_border_span(screen, base, 7);
	}
	if (border.border_part_38 & 2) != 0 {
		fill_border_span(screen, base + 7, 1);
	}
}

/*
The 40-column left edge is evaluated in two four-pixel halves. Retaining both decisions reproduces partial side-border openings caused by a CSEL write between the two comparator phases.
*/
#[inline(always)]
pub fn render_40_left_first(screen: &mut Screen, border: &BorderUnit, cycle: u16) {
	if (border.border_part_40 & 1) == 0 { return; }
	let Some(base) = span_base(cycle, 4) else { return; };
	fill_border_span(screen, base, 4);
}

#[inline(always)]
/* Draw the second four-pixel half of the 40-column left edge after its later comparator decision. */
pub fn render_40_left_second(screen: &mut Screen, border: &BorderUnit, cycle: u16) {
	if (border.border_part_40 & 2) == 0 { return; }
	let Some(base) = span_base(cycle, 0) else { return; };
	fill_border_span(screen, base, 4);
}