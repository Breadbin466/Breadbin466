// =======================================================
// src/cia/constants.rs — MOS 6526A register, control and interrupt constants
// =======================================================

/* The 6526 exposes sixteen registers selected by RS0 through RS3, from port A at offset 0 through control register B at offset 15 (MOS-6526-1981, Register Map). */
pub const PRA: u8 = 0x00;
pub const PRB: u8 = 0x01;
pub const DDRA: u8 = 0x02;
pub const DDRB: u8 = 0x03;
pub const TALO: u8 = 0x04;
pub const TAHI: u8 = 0x05;
pub const TBLO: u8 = 0x06;
pub const TBHI: u8 = 0x07;
pub const TOD10THS: u8 = 0x08;
pub const TODSEC: u8 = 0x09;
pub const TODMIN: u8 = 0x0A;
pub const TODHR: u8 = 0x0B;
pub const SDR: u8 = 0x0C;
pub const ICR: u8 = 0x0D;
pub const CRA: u8 = 0x0E;
pub const CRB: u8 = 0x0F;

/* CRA and CRB bits 0 through 4 select timer start, port-B output, output mode, run mode and forced latch loading (MOS-6526-1981, Control Registers). */
pub const CR_START: u8 = 0x01;
pub const CR_PBON: u8 = 0x02;
pub const CR_OUTMODE: u8 = 0x04;
pub const CR_RUNMODE: u8 = 0x08;
pub const CR_LOAD: u8 = 0x10;

/* CRA selects the Timer A input source, serial-port direction and 50/60 Hz TOD input convention in bits 5 through 7 (MOS-6526-1981, Control Register A). */
pub const CRA_INMODE: u8 = 0x20;
pub const CRA_SPMODE: u8 = 0x40;
pub const CRA_TODIN: u8 = 0x80;

/* CRB bits 5 and 6 select the Timer B count source, while bit 7 selects TOD clock or alarm register writes (MOS-6526-1981, Control Register B). */
pub const CRB_INMODE_MASK: u8 = 0x60;
pub const CRB_ALARM: u8 = 0x80;

/* ICR bits 0 through 4 identify Timer A, Timer B, TOD alarm, serial-port and FLAG interrupt sources; bit 7 reports an active enabled interrupt (MOS-6526-1981, Interrupt Control Register). */
pub const ICR_TA: u8 = 0x01;
pub const ICR_TB: u8 = 0x02;
pub const ICR_ALRM: u8 = 0x04;
pub const ICR_SP: u8 = 0x08;
pub const ICR_FLAG: u8 = 0x10;
pub const ICR_IR: u8 = 0x80;

/* CIA 2 port-A bits 3 through 5 carry the C64 serial-bus ATN, clock and data output controls (C64-PRG-1982, CIA 2 and serial-bus port assignments). */
pub const IEC_OUTPUT_MASK: u8 = 0x38;