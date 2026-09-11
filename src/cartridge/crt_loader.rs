// =======================================================
// src/cartridge/crt_loader.rs — CRT image parser
// =======================================================

use super::constants::CRT_MAGIC;

use super::mapper_interface::{ChipType, MapperType};
/* CrtChip is a borrowed view of one CHIP packet. The packet retains its declared storage type, bank number and load address so mapper-specific placement remains possible after parsing. */
pub struct CrtChip<'a> {
	pub chip_type: ChipType,
	pub bank: usize,
	pub address: u16,
	pub data: &'a [u8],
}

/* CrtImage separates the global CRT header from its ordered CHIP packets. Parsing validates every length before exposing borrowed slices, so mapper construction never receives truncated packet data. */
pub struct CrtImage<'a> {
	pub mapper_type: MapperType,
	pub hardware_revision: u8,
	pub name: String,
	pub game: bool,
	pub exrom: bool,
	pub chips: Vec<CrtChip<'a>>,
}

impl<'a> CrtImage<'a> {
	/* Parsing is transactional: the complete header and every packet are checked before the image is returned, preventing a partially mounted cartridge from replacing the current device. */
	pub fn parse(data: &'a [u8]) -> Result<Self, String> {
		if data.len() < 0x40 || data.get(..16) != Some(CRT_MAGIC.as_slice()) {
			return Err("Invalid CRT header".to_string());
		}

		let header_length = read_u32(data, 0x10)? as usize;
		let header_length = if header_length >= 0x40 {
			header_length
		} else if data.get(0x40..0x44) == Some(b"CHIP") {
			0x40
		} else {
			return Err("Invalid CRT header length".to_string());
		};
		if header_length > data.len() {
			return Err("Truncated CRT header".to_string());
		}

		let hardware_id = read_u16(data, 0x16)?;
		let name = read_name(data.get(0x20..0x40).ok_or("Invalid CRT name field")?);
		let mut mapper_type = MapperType::from_crt_id(hardware_id)
			.ok_or_else(|| format!("Unsupported CRT hardware type {}", hardware_id))?;
		if mapper_type == MapperType::EasyFlashXbank
			&& name.to_ascii_uppercase().contains("EASYFLASH 3")
		{
			mapper_type = MapperType::EasyFlash3;
		}

		/* CRT 1.01 identifies Nordic Replay through hardware revision 1
		 * at header offset $1A (CRT-FORMAT). */
		let hardware_revision = if read_u16(data, 0x14)? >= 0x0101 {
			data[0x1A]
		} else {
			0
		};
		if mapper_type == MapperType::RetroReplay && hardware_revision > 1 {
			return Err(format!(
				"Unsupported Replay hardware revision {}",
				hardware_revision
			));
		}

		let mut chips = Vec::new();
		let mut cursor = header_length;
		while cursor < data.len() {
			if data.get(cursor..cursor + 4) != Some(b"CHIP") {
				return Err("Invalid CRT CHIP packet".to_string());
			}
			let packet_length = read_u32(data, cursor + 4)? as usize;
			if packet_length < 0x10 {
				return Err("Invalid CRT CHIP packet length".to_string());
			}
			let packet_end = cursor
				.checked_add(packet_length)
				.filter(|end| *end <= data.len())
				.ok_or("Truncated CRT CHIP packet")?;
			let chip_type = ChipType::try_from(read_u16(data, cursor + 8)?)
				.map_err(|_| "Unsupported CRT CHIP type".to_string())?;
			let bank = read_u16(data, cursor + 0x0A)? as usize;
			let address = read_u16(data, cursor + 0x0C)?;
			let payload_length = read_u16(data, cursor + 0x0E)? as usize;
			if payload_length == 0 || payload_length > packet_length - 0x10 {
				return Err("Invalid CRT CHIP payload length".to_string());
			}
			let payload_end = cursor
				.checked_add(0x10 + payload_length)
				.filter(|end| *end <= packet_end)
				.ok_or("Truncated CRT CHIP payload")?;
			chips.push(CrtChip {
				chip_type,
				bank,
				address,
				data: &data[cursor + 0x10..payload_end],
			});
			cursor = packet_end;
		}

		if chips.is_empty() {
			return Err("CRT contains no CHIP packets".to_string());
		}

		let mut game = data[0x19] != 0;
		let mut exrom = data[0x18] != 0;
		if mapper_type == MapperType::Normal {
			let has_roml = chips.iter().any(|chip| chip.address == 0x8000);
			let has_romh_16k = chips.iter().any(|chip| chip.address == 0xA000);
			let has_romh_ultimax = chips.iter().any(|chip| chip.address == 0xE000);
			let roml_size: usize = chips
				.iter()
				.filter(|chip| chip.address == 0x8000)
				.map(|chip| chip.data.len())
				.sum();

			if has_roml && roml_size <= 0x2000 && !has_romh_16k && !has_romh_ultimax {
				game = true;
				exrom = false;
			} else if has_roml && has_romh_16k {
				game = false;
				exrom = false;
			} else if has_roml && has_romh_ultimax {
				game = false;
				exrom = true;
			}
		}

		Ok(Self {
			mapper_type,
			hardware_revision,
			name,
			game,
			exrom,
			chips,
		})
	}
}

fn read_name(bytes: &[u8]) -> String {
	let end = bytes
		.iter()
		.position(|byte| *byte == 0)
		.unwrap_or(bytes.len());
	String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, String> {
	let bytes = data
		.get(offset..offset + 2)
		.ok_or_else(|| "Truncated CRT field".to_string())?;
	Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, String> {
	let bytes = data
		.get(offset..offset + 4)
		.ok_or_else(|| "Truncated CRT field".to_string())?;
	Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}