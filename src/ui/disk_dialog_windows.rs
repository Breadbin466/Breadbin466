// =======================================================
// src/ui/disk_dialog_windows.rs — Native Windows disk-image dialogs
// =======================================================

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::SetWindowTheme;
use windows_sys::Win32::UI::WindowsAndMessaging::{
	BM_GETCHECK, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, DLGTEMPLATE, DialogBoxIndirectParamW,
	EndDialog, GWLP_USERDATA, GetDlgItem, GetDlgItemTextW, GetWindowLongPtrW, SendDlgItemMessageW,
	SetDlgItemTextW, SetWindowLongPtrW, WM_CLOSE, WM_COMMAND, WM_INITDIALOG,
};

use crate::fdd1541::disk_image::{ImageFormat, format_from_path};

/*
Convert Image uses two native Windows stages.

Stage 1 is the Windows Common Item Dialog, through wfd, and selects only the
source image.

Stage 2 is a compact modal Win32 dialog managed by the Windows dialog manager.
Its widgets are genuine standard Windows controls: STATIC, EDIT, COMBOBOX and
BUTTON. When Windows is using the dark application theme, the dialog asks DWM
for the native dark title bar, applies the Windows dark control themes, and
paints only the dialog/control backgrounds needed by classic Win32. No WinUI,
XAML, custom widget implementation, manifest, build script or Cargo linker
configuration is involved.
*/

#[derive(Clone, Debug)]
pub(crate) struct ConvertRequest {
	pub source: PathBuf,
	pub destination: PathBuf,
	pub destination_format: ImageFormat,
	pub reclaim_space: bool,
	pub delete_source: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct CreateRequest {
	pub destination: PathBuf,
	pub format: ImageFormat,
}

const ID_CONVERT: i32 = 1;
const ID_CANCEL: i32 = 2;
const ID_SOURCE_VALUE: i32 = 2101;
const ID_DESTINATION_VALUE: i32 = 2102;
const ID_SAVE_AS: i32 = 2103;
const ID_FORMAT: i32 = 2104;
const ID_RECLAIM: i32 = 2105;
const ID_DELETE_SOURCE: i32 = 2106;

const BST_CHECKED_VALUE: isize = 1;

type GdiHandle = *mut c_void;

const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
const HKEY_CURRENT_USER_VALUE: isize = 0x8000_0001u32 as isize;

const WM_CTLCOLOREDIT_VALUE: u32 = 0x0133;
const WM_CTLCOLORBTN_VALUE: u32 = 0x0135;
const WM_CTLCOLORDLG_VALUE: u32 = 0x0136;
const WM_CTLCOLORSTATIC_VALUE: u32 = 0x0138;

const DARK_BACKGROUND: u32 = 0x0020_2020;
const DARK_FIELD: u32 = 0x002B_2B2B;
const DARK_TEXT: u32 = 0x00F2_F2F2;

#[link(name = "dwmapi")]
unsafe extern "system" {
	fn DwmSetWindowAttribute(
		hwnd: HWND,
		dw_attribute: u32,
		pv_attribute: *const c_void,
		cb_attribute: u32,
	) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
	fn RegGetValueW(
		hkey: *mut c_void,
		lp_sub_key: *const u16,
		lp_value: *const u16,
		dw_flags: u32,
		pdw_type: *mut u32,
		pv_data: *mut c_void,
		pcb_data: *mut u32,
	) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
	fn CreateSolidBrush(color: u32) -> GdiHandle;
	fn DeleteObject(object: GdiHandle) -> i32;
	fn SetTextColor(hdc: GdiHandle, color: u32) -> u32;
	fn SetBkColor(hdc: GdiHandle, color: u32) -> u32;
}

fn windows_apps_use_dark_mode() -> bool {
	unsafe {
		let subkey = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
		let value_name = wide("AppsUseLightTheme");
		let mut value: u32 = 1;
		let mut size = std::mem::size_of::<u32>() as u32;
		let status = RegGetValueW(
			HKEY_CURRENT_USER_VALUE as *mut c_void,
			subkey.as_ptr(),
			value_name.as_ptr(),
			RRF_RT_REG_DWORD,
			null_mut(),
			(&mut value as *mut u32).cast::<c_void>(),
			&mut size,
		);
		status == 0 && value == 0
	}
}

const DS_SHELLFONT_STYLE: u32 = 0x0048;
const DS_MODALFRAME_STYLE: u32 = 0x0080;
const DS_CENTER_STYLE: u32 = 0x0800;
const WS_POPUP_STYLE: u32 = 0x8000_0000;
const WS_CAPTION_STYLE: u32 = 0x00c0_0000;
const WS_SYSMENU_STYLE: u32 = 0x0008_0000;
const WS_CHILD_STYLE: u32 = 0x4000_0000;
const WS_VISIBLE_STYLE: u32 = 0x1000_0000;
const WS_TABSTOP_STYLE: u32 = 0x0001_0000;
const WS_BORDER_STYLE: u32 = 0x0080_0000;
const WS_VSCROLL_STYLE: u32 = 0x0020_0000;
const ES_AUTOHSCROLL_STYLE: u32 = 0x0080;
const CBS_DROPDOWNLIST_STYLE: u32 = 0x0003;
const BS_DEFPUSHBUTTON_STYLE: u32 = 0x0001;
const BS_AUTOCHECKBOX_STYLE: u32 = 0x0003;
const SS_LEFTNOWORDWRAP_STYLE: u32 = 0x000c;

const CLASS_BUTTON: u16 = 0x0080;
const CLASS_EDIT: u16 = 0x0081;
const CLASS_STATIC: u16 = 0x0082;
const CLASS_COMBOBOX: u16 = 0x0085;

struct OptionsState {
	source: PathBuf,
	destination_dir: PathBuf,
	result: Option<ConvertRequest>,
	dark_mode: bool,
	background_brush: GdiHandle,
	field_brush: GdiHandle,
}

fn report_backend_error(error: &str) {
	let _ = rfd::MessageDialog::new()
		.set_title("Disk Image Tools")
		.set_description(error)
		.set_level(rfd::MessageLevel::Error)
		.set_buttons(rfd::MessageButtons::Ok)
		.show();
}

fn wide(text: &str) -> Vec<u16> {
	text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn format_from_index(index: usize) -> ImageFormat {
	match index {
		0 => ImageFormat::D64,
		1 => ImageFormat::D7z,
		2 => ImageFormat::G64,
		3 => ImageFormat::Nib,
		_ => ImageFormat::Nbz,
	}
}

fn select_source_image() -> Result<Option<PathBuf>, String> {
	let params = wfd::DialogParams {
		title: "Select Image to Convert",
		ok_button_label: "Next",
		file_types: vec![("Disk images", "*.d64;*.d7z;*.g64;*.nib;*.nbz")],
		file_type_index: 1,
		options: wfd::FOS_FORCEFILESYSTEM | wfd::FOS_FILEMUSTEXIST | wfd::FOS_PATHMUSTEXIST,
		..Default::default()
	};

	match wfd::open_dialog(params) {
		Ok(result) => {
			let source = result.selected_file_path;
			if format_from_path(&source).is_none() {
				return Err("The selected file is not a supported disk image.".to_string());
			}
			Ok(Some(source))
		}
		Err(wfd::DialogError::UserCancelled) => Ok(None),
		Err(error) => Err(format!("{error:?}")),
	}
}

/*
DLGTEMPLATEEX is a variable-length structure. windows-sys exposes the Win32
entry point but not a safe builder, so the template is assembled according to
the documented WORD/DWORD layout and kept DWORD-aligned.
*/
struct DialogTemplate {
	storage: Vec<u32>,
	len: usize,
}

impl DialogTemplate {
	fn new(capacity_bytes: usize) -> Self {
		Self {
			storage: vec![0; capacity_bytes.div_ceil(4)],
			len: 0,
		}
	}

	fn ensure(&mut self, additional: usize) {
		let required = self.len + additional;
		let words = required.div_ceil(4);
		if words > self.storage.len() {
			self.storage.resize(words.next_power_of_two(), 0);
		}
	}

	fn push_u8(&mut self, value: u8) {
		self.ensure(1);
		unsafe {
			(self.storage.as_mut_ptr().cast::<u8>().add(self.len)).write(value);
		}
		self.len += 1;
	}

	fn push_u16(&mut self, value: u16) {
		self.ensure(2);
		unsafe {
			self.storage
				.as_mut_ptr()
				.cast::<u8>()
				.add(self.len)
				.cast::<u16>()
				.write_unaligned(value);
		}
		self.len += 2;
	}

	fn push_i16(&mut self, value: i16) {
		self.push_u16(value as u16);
	}

	fn push_u32(&mut self, value: u32) {
		self.ensure(4);
		unsafe {
			self.storage
				.as_mut_ptr()
				.cast::<u8>()
				.add(self.len)
				.cast::<u32>()
				.write_unaligned(value);
		}
		self.len += 4;
	}

	fn push_wide(&mut self, value: &str) {
		for unit in value.encode_utf16() {
			self.push_u16(unit);
		}
		self.push_u16(0);
	}

	fn align_dword(&mut self) {
		while self.len % 4 != 0 {
			self.push_u8(0);
		}
	}

	fn push_class_ordinal(&mut self, ordinal: u16) {
		self.push_u16(0xffff);
		self.push_u16(ordinal);
	}

	#[allow(clippy::too_many_arguments)]
	fn push_item(
		&mut self,
		id: i32,
		class_ordinal: u16,
		text: &str,
		style: u32,
		x: i16,
		y: i16,
		cx: i16,
		cy: i16,
	) {
		self.align_dword();
		self.push_u32(0);
		self.push_u32(0);
		self.push_u32(style);
		self.push_i16(x);
		self.push_i16(y);
		self.push_i16(cx);
		self.push_i16(cy);
		self.push_u32(id as u32);
		self.push_class_ordinal(class_ordinal);
		self.push_wide(text);
		self.push_u16(0);
	}

	fn as_ptr(&self) -> *const DLGTEMPLATE {
		self.storage.as_ptr().cast::<DLGTEMPLATE>()
	}
}

fn build_options_template() -> DialogTemplate {
	let mut template = DialogTemplate::new(4096);

	template.push_u16(1);
	template.push_u16(0xffff);
	template.push_u32(0);
	template.push_u32(0);
	template.push_u32(
		WS_POPUP_STYLE
			| WS_CAPTION_STYLE
			| WS_SYSMENU_STYLE
			| DS_MODALFRAME_STYLE
			| DS_CENTER_STYLE
			| DS_SHELLFONT_STYLE,
	);
	template.push_u16(12);
	template.push_i16(0);
	template.push_i16(0);
	template.push_i16(340);
	template.push_i16(190);
	template.push_u16(0);
	template.push_u16(0);
	template.push_wide("Convert Image");

	template.push_u16(9);
	template.push_u16(400);
	template.push_u8(0);
	template.push_u8(1);
	template.push_wide("MS Shell Dlg");

	let static_style = WS_CHILD_STYLE | WS_VISIBLE_STYLE | SS_LEFTNOWORDWRAP_STYLE;
	let input_style = WS_CHILD_STYLE | WS_VISIBLE_STYLE | WS_TABSTOP_STYLE;

	const LABEL_X: i16 = 20;
	const LABEL_W: i16 = 78;
	const VALUE_X: i16 = 104;
	const VALUE_W: i16 = 216;
	const TEXT_H: i16 = 10;
	const FIELD_H: i16 = 8;

	template.push_item(
		0,
		CLASS_STATIC,
		"Source:",
		static_style,
		LABEL_X,
		20,
		LABEL_W,
		TEXT_H,
	);
	template.push_item(
		ID_SOURCE_VALUE,
		CLASS_STATIC,
		"",
		static_style,
		VALUE_X,
		20,
		VALUE_W,
		TEXT_H,
	);

	template.push_item(
		0,
		CLASS_STATIC,
		"Destination:",
		static_style,
		LABEL_X,
		42,
		LABEL_W,
		TEXT_H,
	);
	template.push_item(
		ID_DESTINATION_VALUE,
		CLASS_STATIC,
		"",
		static_style,
		VALUE_X,
		42,
		VALUE_W,
		TEXT_H,
	);

	template.push_item(
		0,
		CLASS_STATIC,
		"Save as:",
		static_style,
		LABEL_X,
		66,
		LABEL_W,
		TEXT_H,
	);
	template.push_item(
		ID_SAVE_AS,
		CLASS_EDIT,
		"",
		input_style | WS_BORDER_STYLE | ES_AUTOHSCROLL_STYLE,
		VALUE_X,
		66,
		VALUE_W,
		FIELD_H,
	);

	template.push_item(
		0,
		CLASS_STATIC,
		"Target format:",
		static_style,
		LABEL_X,
		89,
		LABEL_W,
		TEXT_H,
	);
	template.push_item(
		ID_FORMAT,
		CLASS_COMBOBOX,
		"",
		input_style | WS_VSCROLL_STYLE | CBS_DROPDOWNLIST_STYLE,
		VALUE_X,
		85,
		34,
		72,
	);

	template.push_item(
		ID_RECLAIM,
		CLASS_BUTTON,
		"Reclaim space",
		input_style | BS_AUTOCHECKBOX_STYLE,
		LABEL_X,
		112,
		132,
		13,
	);
	template.push_item(
		ID_DELETE_SOURCE,
		CLASS_BUTTON,
		"Delete source image after successful conversion",
		input_style | BS_AUTOCHECKBOX_STYLE,
		LABEL_X,
		128,
		260,
		13,
	);

	template.push_item(
		ID_CANCEL,
		CLASS_BUTTON,
		"Cancel",
		input_style,
		196,
		158,
		58,
		21,
	);
	template.push_item(
		ID_CONVERT,
		CLASS_BUTTON,
		"Convert",
		input_style | BS_DEFPUSHBUTTON_STYLE,
		262,
		158,
		58,
		21,
	);

	template
}

unsafe fn set_dialog_text(dialog: HWND, id: i32, text: &str) {
	unsafe {
		let text = wide(text);
		let _ = SetDlgItemTextW(dialog, id, text.as_ptr());
	}
}

unsafe fn get_dialog_text(dialog: HWND, id: i32) -> String {
	unsafe {
		let mut buffer = vec![0u16; 1024];
		let copied = GetDlgItemTextW(dialog, id, buffer.as_mut_ptr(), buffer.len() as i32);
		String::from_utf16_lossy(&buffer[..copied as usize])
	}
}

unsafe fn apply_windows_theme(dialog: HWND, dark_mode: bool) {
	unsafe {
		if dark_mode {
			let enabled: i32 = 1;
			let _ = DwmSetWindowAttribute(
				dialog,
				DWMWA_USE_IMMERSIVE_DARK_MODE,
				(&enabled as *const i32).cast::<c_void>(),
				std::mem::size_of::<i32>() as u32,
			);

			let explorer = wide("DarkMode_Explorer");
			let cfd = wide("DarkMode_CFD");

			for id in [ID_RECLAIM, ID_DELETE_SOURCE, ID_CANCEL, ID_CONVERT] {
				let control = GetDlgItem(dialog, id);
				if !control.is_null() {
					let _ = SetWindowTheme(control, explorer.as_ptr(), null());
				}
			}

			for id in [ID_SAVE_AS, ID_FORMAT] {
				let control = GetDlgItem(dialog, id);
				if !control.is_null() {
					let _ = SetWindowTheme(control, cfd.as_ptr(), null());
				}
			}
		} else {
			let explorer = wide("Explorer");
			for id in [
				ID_SAVE_AS,
				ID_FORMAT,
				ID_RECLAIM,
				ID_DELETE_SOURCE,
				ID_CANCEL,
				ID_CONVERT,
			] {
				let control = GetDlgItem(dialog, id);
				if !control.is_null() {
					let _ = SetWindowTheme(control, explorer.as_ptr(), null());
				}
			}
		}
	}
}

unsafe fn complete_conversion(dialog: HWND, state: &mut OptionsState) -> bool {
	unsafe {
		let name = get_dialog_text(dialog, ID_SAVE_AS);
		let name = name.trim();
		if name.is_empty() {
			return false;
		}

		let index = SendDlgItemMessageW(dialog, ID_FORMAT, CB_GETCURSEL, 0, 0);
		let destination_format = format_from_index(index.max(0) as usize);
		let mut destination = state.destination_dir.join(name);
		destination.set_extension(destination_format.extension());
		let reclaim_space =
			SendDlgItemMessageW(dialog, ID_RECLAIM, BM_GETCHECK, 0, 0) == BST_CHECKED_VALUE;
		let delete_source =
			SendDlgItemMessageW(dialog, ID_DELETE_SOURCE, BM_GETCHECK, 0, 0) == BST_CHECKED_VALUE;

		state.result = Some(ConvertRequest {
			source: state.source.clone(),
			destination,
			destination_format,
			reclaim_space,
			delete_source,
		});
		true
	}
}

unsafe extern "system" fn options_dialog_proc(
	dialog: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> isize {
	unsafe {
		if message == WM_INITDIALOG {
			let state = lparam as *mut OptionsState;
			SetWindowLongPtrW(dialog, GWLP_USERDATA, state as isize);
			if state.is_null() {
				return 0;
			}

			let state = &mut *state;
			let source_name = state
				.source
				.file_name()
				.map(|value| value.to_string_lossy().into_owned())
				.unwrap_or_default();
			let destination_text = state.destination_dir.display().to_string();
			let stem = state
				.source
				.file_stem()
				.map(|value| value.to_string_lossy().into_owned())
				.unwrap_or_default();

			set_dialog_text(dialog, ID_SOURCE_VALUE, &source_name);
			set_dialog_text(dialog, ID_DESTINATION_VALUE, &destination_text);
			set_dialog_text(dialog, ID_SAVE_AS, &stem);

			for name in ["D64", "D7Z", "G64", "NIB", "NBZ"] {
				let value = wide(name);
				let _ = SendDlgItemMessageW(
					dialog,
					ID_FORMAT,
					CB_ADDSTRING,
					0,
					value.as_ptr() as LPARAM,
				);
			}
			let _ = SendDlgItemMessageW(dialog, ID_FORMAT, CB_SETCURSEL, 1, 0);

			apply_windows_theme(dialog, state.dark_mode);

			return 1;
		}

		let state_ptr = GetWindowLongPtrW(dialog, GWLP_USERDATA) as *mut OptionsState;

		if !state_ptr.is_null() && (*state_ptr).dark_mode {
			let state = &mut *state_ptr;
			match message {
				WM_CTLCOLORDLG_VALUE | WM_CTLCOLORSTATIC_VALUE | WM_CTLCOLORBTN_VALUE => {
					let hdc = wparam as GdiHandle;
					let _ = SetTextColor(hdc, DARK_TEXT);
					let _ = SetBkColor(hdc, DARK_BACKGROUND);
					return state.background_brush as isize;
				}
				WM_CTLCOLOREDIT_VALUE => {
					let hdc = wparam as GdiHandle;
					let _ = SetTextColor(hdc, DARK_TEXT);
					let _ = SetBkColor(hdc, DARK_FIELD);
					return state.field_brush as isize;
				}
				_ => {}
			}
		}

		match message {
			WM_COMMAND if !state_ptr.is_null() => {
				let id = (wparam & 0xffff) as i32;
				match id {
					ID_CONVERT => {
						if complete_conversion(dialog, &mut *state_ptr) {
							let _ = EndDialog(dialog, ID_CONVERT as isize);
						}
						1
					}
					ID_CANCEL => {
						let _ = EndDialog(dialog, ID_CANCEL as isize);
						1
					}
					_ => 0,
				}
			}
			WM_CLOSE => {
				let _ = EndDialog(dialog, ID_CANCEL as isize);
				1
			}
			_ => 0,
		}
	}
}

unsafe fn show_options_dialog(source: PathBuf) -> Result<Option<ConvertRequest>, String> {
	unsafe {
		let instance = GetModuleHandleW(null());
		if instance.is_null() {
			return Err("Windows could not obtain the application module handle.".to_string());
		}

		let Some(destination_dir) = source.parent().map(std::path::Path::to_path_buf) else {
			return Err("The source image has no destination directory.".to_string());
		};

		let dark_mode = windows_apps_use_dark_mode();
		let background_brush = if dark_mode {
			CreateSolidBrush(DARK_BACKGROUND)
		} else {
			null_mut()
		};
		let field_brush = if dark_mode {
			CreateSolidBrush(DARK_FIELD)
		} else {
			null_mut()
		};

		let mut state = OptionsState {
			source,
			destination_dir,
			result: None,
			dark_mode,
			background_brush,
			field_brush,
		};
		let template = build_options_template();
		let result = DialogBoxIndirectParamW(
			instance,
			template.as_ptr(),
			null_mut_hwnd(),
			Some(options_dialog_proc),
			(&mut state as *mut OptionsState) as LPARAM,
		);
		if !background_brush.is_null() {
			let _ = DeleteObject(background_brush);
		}
		if !field_brush.is_null() {
			let _ = DeleteObject(field_brush);
		}

		if result == -1 {
			return Err("Windows could not create the conversion-options dialog.".to_string());
		}

		Ok(state.result)
	}
}

const fn null_mut_hwnd() -> HWND {
	std::ptr::null_mut()
}

pub(crate) fn convert_image() -> Option<ConvertRequest> {
	let source = match select_source_image() {
		Ok(Some(source)) => source,
		Ok(None) => return None,
		Err(error) => {
			report_backend_error(&format!(
				"Windows could not open the source-image dialog:\n\n{error}"
			));
			return None;
		}
	};

	match unsafe { show_options_dialog(source) } {
		Ok(result) => result,
		Err(error) => {
			report_backend_error(&error);
			None
		}
	}
}

pub(crate) fn create_image() -> Option<CreateRequest> {
	let params = wfd::DialogParams {
		title: "Create New Disk",
		file_name: "untitled",
		file_name_label: "Save as:",
		ok_button_label: "Create",
		file_types: vec![
			("D64", "*.d64"),
			("D7Z", "*.d7z"),
			("G64", "*.g64"),
			("NIB", "*.nib"),
			("NBZ", "*.nbz"),
		],
		file_type_index: 1,
		default_extension: "d64",
		options: wfd::FOS_FORCEFILESYSTEM | wfd::FOS_PATHMUSTEXIST | wfd::FOS_OVERWRITEPROMPT,
		..Default::default()
	};

	match wfd::save_dialog(params) {
		Ok(result) => {
			let format = format_from_index(result.selected_filter_index.saturating_sub(1) as usize);
			let mut destination = result.selected_file_path;
			destination.set_extension(format.extension());
			Some(CreateRequest {
				destination,
				format,
			})
		}
		Err(wfd::DialogError::UserCancelled) => None,
		Err(error) => {
			report_backend_error(&format!(
				"Windows could not open the Create New Disk dialog:\n\n{error:?}"
			));
			None
		}
	}
}