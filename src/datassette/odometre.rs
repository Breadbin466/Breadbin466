// =======================================================
// src/datassette/odometre.rs — Tape odometre kinematics
// =======================================================

use super::constants::{
	C64_PAL_CYCLES_PER_SECOND, ODOMETRE_INITIAL_REEL_RADIUS_METRES, ODOMETRE_MAX_VALUE,
	ODOMETRE_TAPE_SPEED_METRES_PER_SECOND, ODOMETRE_TAPE_THICKNESS_METRES,
	ODOMETRE_TURNS_PER_COUNTER_UNIT,
};

/* Odometre models the three-digit mechanical counter from tape travel. As tape accumulates on the take-up reel, its increasing radius changes turns per unit length and therefore the counter rate. */
pub struct Odometre {
	/* Empty take-up reel radius. */
	r0: f64,
	/* Nominal linear tape speed while the motor is moving the transport. */
	v: f64,
	/* Tape thickness converts wound length into added reel cross-sectional area. */
	thickness: f64,
}

impl Odometre {
	/* The model is parameterised by the empty reel radius, nominal tape speed and tape thickness fixed for the reference transport. */
	pub fn new() -> Self {
		Self {
			r0: ODOMETRE_INITIAL_REEL_RADIUS_METRES,
			v: ODOMETRE_TAPE_SPEED_METRES_PER_SECOND,
			thickness: ODOMETRE_TAPE_THICKNESS_METRES,
		}
	}

	/* Elapsed motor cycles become tape length, then accumulated reel area, reel turns and finally counter units. The square-root radius term accounts for tape winding onto an ever larger reel. */
	pub fn calculate_value(&self, motor_cycles: u64) -> f64 {
		let current_t = motor_cycles as f64 / C64_PAL_CYCLES_PER_SECOND;

		/* Wound tape adds annular area linearly with travelled length; solving that area for radius produces the square-root term. */
		let r_t = (self.r0 * self.r0
			+ (self.v * self.thickness * current_t) / std::f64::consts::PI)
			.sqrt();
		/* Dividing the accumulated annular area by tape cross-section yields the number of layers and therefore reel revolutions. */
		let total_turns =
			(std::f64::consts::PI * (r_t * r_t - self.r0 * self.r0)) / (self.v * self.thickness);
		let counter_units = total_turns / ODOMETRE_TURNS_PER_COUNTER_UNIT;

		counter_units.clamp(0.0, ODOMETRE_MAX_VALUE)
	}
}