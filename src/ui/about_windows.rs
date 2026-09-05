// =======================================================
// src/ui/about_windows.rs — Native Win32 About dialog backend
// =======================================================

use crate::ui::constants::{
	CLIENT_HEIGHT, CLIENT_WIDTH, ID_COPYRIGHT, ID_DESCRIPTION, ID_ICON, ID_NAME, ID_OK, ID_VERSION,
	MARGIN, SS_ICON_STYLE, SS_LEFT_STYLE, SUBTLE_COLOUR,
};
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
	CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, CreateFontW, DEFAULT_CHARSET,
	DeleteObject, GetSysColorBrush, HBRUSH, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, SetBkMode,
	SetTextColor, TRANSPARENT, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
	AdjustWindowRectEx, BS_DEFPUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow,
	GetDlgCtrlID, GetDlgItem, GetWindowRect, HICON, HMENU, ICON_BIG, IDC_ARROW, IDI_APPLICATION,
	IsWindow, LoadCursorW, LoadIconW, RegisterClassW, STM_SETICON, SW_SHOW, SWP_NOSIZE,
	SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, WINDOW_STYLE,
	WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY, WM_GETICON, WM_SETFONT, WNDCLASSW,
	WS_CAPTION, WS_CHILD, WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};
use winit::window::Window;

use super::about::{APP_NAME, COPYRIGHT, VERSION, description};
use crate::emulator::Result;

/* Win32 does not provide the required rich About panel directly, so this backend owns one modeless top-level window, its DPI-scaled fonts and their explicit GDI lifetime. */
struct AboutState {
	hwnd: isize,
	fonts: Vec<isize>,
}

static STATE: OnceLock<Mutex<AboutState>> = OnceLock::new();
static CLASS: OnceLock<bool> = OnceLock::new();

fn state() -> &'static Mutex<AboutState> {
	STATE.get_or_init(|| {
		Mutex::new(AboutState {
			hwnd: 0,
			fonts: Vec::new(),
		})
	})
}

/* Repeated requests foreground the existing modeless dialog instead of creating duplicate native windows. */
pub fn show(window: &Window) -> Result<()> {
	let handle = window.window_handle()?;
	let parent = match handle.as_raw() {
		RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut c_void),
		_ => return Err("The winit window does not expose a Win32 handle".into()),
	};

	if let Ok(current) = state().lock() {
		if current.hwnd != 0 {
			let existing = HWND(current.hwnd as *mut c_void);
			if unsafe { IsWindow(Some(existing)) }.as_bool() {
				unsafe {
					let _ = SetForegroundWindow(existing);
				}
				return Ok(());
			}
		}
	}

	unsafe { create(parent) }
}

/* Window creation is ordered as class registration, DPI-aware frame calculation, control construction, icon inheritance and finally publication in the shared state. */
unsafe fn create(parent: HWND) -> Result<()> {
	unsafe {
		let module = GetModuleHandleW(None)?;
		let instance = HINSTANCE(module.0);

		register_class(instance)?;

		let dpi = GetDpiForWindow(parent).max(96);
		let scale = |value: i32| -> i32 { (value * dpi as i32) / 96 };

		let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
		let ex_style = WS_EX_DLGMODALFRAME;

		let mut frame = RECT {
			left: 0,
			top: 0,
			right: scale(CLIENT_WIDTH),
			bottom: scale(CLIENT_HEIGHT),
		};
		AdjustWindowRectEx(&mut frame, style, false, ex_style)?;
		let width = frame.right - frame.left;
		let height = frame.bottom - frame.top;

		let title = wide(&format!("About {APP_NAME}"));
		let hwnd = CreateWindowExW(
			ex_style,
			w!("Breadbin466AboutWindow"),
			PCWSTR(title.as_ptr()),
			style,
			0,
			0,
			width,
			height,
			Some(parent),
			None,
			Some(instance),
			None,
		)?;

		centre_on_parent(hwnd, parent, width, height);

		let title_font = create_font(dpi, 16, 600);
		let body_font = create_font(dpi, 9, 400);

		build_controls(hwnd, instance, dpi, &scale, title_font, body_font)?;
		apply_icon(hwnd, parent);

		if let Ok(mut current) = state().lock() {
			current.hwnd = hwnd.0 as isize;
			current.fonts = vec![title_font.0 as isize, body_font.0 as isize];
		}

		let _ = ShowWindow(hwnd, SW_SHOW);
		let _ = UpdateWindow(hwnd);
		let _ = SetForegroundWindow(hwnd);
		Ok(())
	}
}

unsafe fn build_controls(
	hwnd: HWND,
	instance: HINSTANCE,
	dpi: u32,
	scale: &dyn Fn(i32) -> i32,
	title_font: HFONT,
	body_font: HFONT,
) -> Result<()> {
	unsafe {
		let icon_size = (dpi as i32 * 64) / 96;

		let icon = CreateWindowExW(
			Default::default(),
			w!("STATIC"),
			PCWSTR::null(),
			WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_ICON_STYLE),
			scale(MARGIN),
			scale(MARGIN),
			icon_size,
			icon_size,
			Some(hwnd),
			Some(HMENU(ID_ICON as isize as *mut c_void)),
			Some(instance),
			None,
		)?;
		let _ = icon;

		let text_left = scale(MARGIN) + icon_size + scale(20);
		let text_width = scale(CLIENT_WIDTH - MARGIN) - text_left;

		let name = label(
			hwnd,
			instance,
			ID_NAME,
			APP_NAME,
			text_left,
			scale(MARGIN),
			text_width,
			scale(30),
		)?;
		SendMessageW(
			name,
			WM_SETFONT,
			Some(WPARAM(title_font.0 as usize)),
			Some(LPARAM(1)),
		);

		let version_text = format!("Version {VERSION}");
		let version = label(
			hwnd,
			instance,
			ID_VERSION,
			&version_text,
			text_left,
			scale(MARGIN + 34),
			text_width,
			scale(20),
		)?;
		SendMessageW(
			version,
			WM_SETFONT,
			Some(WPARAM(body_font.0 as usize)),
			Some(LPARAM(1)),
		);

		let body_text = description("\r\n\r\n");
		let body = label(
			hwnd,
			instance,
			ID_DESCRIPTION,
			&body_text,
			scale(MARGIN),
			scale(MARGIN + 96),
			scale(CLIENT_WIDTH - MARGIN * 2),
			scale(252),
		)?;
		SendMessageW(
			body,
			WM_SETFONT,
			Some(WPARAM(body_font.0 as usize)),
			Some(LPARAM(1)),
		);

		let copyright = label(
			hwnd,
			instance,
			ID_COPYRIGHT,
			COPYRIGHT,
			scale(MARGIN),
			scale(CLIENT_HEIGHT - 74),
			scale(CLIENT_WIDTH - MARGIN * 2),
			scale(20),
		)?;
		SendMessageW(
			copyright,
			WM_SETFONT,
			Some(WPARAM(body_font.0 as usize)),
			Some(LPARAM(1)),
		);

		let ok_text = wide("OK");
		let ok = CreateWindowExW(
			Default::default(),
			w!("BUTTON"),
			PCWSTR(ok_text.as_ptr()),
			WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
			scale(CLIENT_WIDTH - MARGIN - 96),
			scale(CLIENT_HEIGHT - 44),
			scale(96),
			scale(28),
			Some(hwnd),
			Some(HMENU(ID_OK as isize as *mut c_void)),
			Some(instance),
			None,
		)?;
		SendMessageW(
			ok,
			WM_SETFONT,
			Some(WPARAM(body_font.0 as usize)),
			Some(LPARAM(1)),
		);

		Ok(())
	}
}

unsafe fn label(
	parent: HWND,
	instance: HINSTANCE,
	id: i32,
	text: &str,
	x: i32,
	y: i32,
	width: i32,
	height: i32,
) -> Result<HWND> {
	unsafe {
		let buffer = wide(text);
		let hwnd = CreateWindowExW(
			Default::default(),
			w!("STATIC"),
			PCWSTR(buffer.as_ptr()),
			WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_LEFT_STYLE),
			x,
			y,
			width,
			height,
			Some(parent),
			Some(HMENU(id as isize as *mut c_void)),
			Some(instance),
			None,
		)?;
		Ok(hwnd)
	}
}

unsafe fn apply_icon(hwnd: HWND, parent: HWND) {
	unsafe {
		let inherited = SendMessageW(
			parent,
			WM_GETICON,
			Some(WPARAM(ICON_BIG as usize)),
			Some(LPARAM(0)),
		);
		let icon = if inherited.0 != 0 {
			HICON(inherited.0 as *mut c_void)
		} else {
			match LoadIconW(None, IDI_APPLICATION) {
				Ok(fallback) => fallback,
				Err(_) => return,
			}
		};

		if let Ok(target) = GetDlgItem(Some(hwnd), ID_ICON) {
			SendMessageW(
				target,
				STM_SETICON,
				Some(WPARAM(icon.0 as usize)),
				Some(LPARAM(0)),
			);
		}
	}
}

unsafe fn centre_on_parent(hwnd: HWND, parent: HWND, width: i32, height: i32) {
	unsafe {
		let mut bounds = RECT::default();
		if GetWindowRect(parent, &mut bounds).is_err() {
			return;
		}
		let x = bounds.left + ((bounds.right - bounds.left) - width) / 2;
		let y = bounds.top + ((bounds.bottom - bounds.top) - height) / 2;
		let _ = SetWindowPos(
			hwnd,
			None,
			x.max(0),
			y.max(0),
			0,
			0,
			SWP_NOZORDER | SWP_NOSIZE,
		);
	}
}

/* The native class is registered once per process; subsequent dialogs reuse the same window procedure and background resources. */
unsafe fn register_class(instance: HINSTANCE) -> Result<()> {
	unsafe {
		if *CLASS.get_or_init(|| {
			let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
			let class = WNDCLASSW {
				lpfnWndProc: Some(about_window_proc),
				hInstance: instance,
				hCursor: cursor,
				hbrBackground: GetSysColorBrush(COLOR_WINDOW),
				lpszClassName: w!("Breadbin466AboutWindow"),
				..Default::default()
			};
			RegisterClassW(&class) != 0
		}) {
			return Ok(());
		}
		Err("Failed to register the About window class".into())
	}
}

unsafe fn create_font(dpi: u32, points: i32, weight: i32) -> HFONT {
	unsafe {
		let height = -((points * dpi as i32) / 72);
		CreateFontW(
			height,
			0,
			0,
			0,
			weight,
			0,
			0,
			0,
			DEFAULT_CHARSET,
			OUT_DEFAULT_PRECIS,
			CLIP_DEFAULT_PRECIS,
			CLEARTYPE_QUALITY,
			0,
			w!("Segoe UI"),
		)
	}
}

unsafe extern "system" fn about_window_proc(
	hwnd: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> LRESULT {
	unsafe {
		match message {
			WM_CTLCOLORSTATIC => {
				let hdc = HDC(wparam.0 as *mut c_void);
				let control = HWND(lparam.0 as *mut c_void);
				SetBkMode(hdc, TRANSPARENT);
				let id = GetDlgCtrlID(control);
				if id == ID_VERSION || id == ID_COPYRIGHT {
					SetTextColor(hdc, COLORREF(SUBTLE_COLOUR));
				}
				let brush: HBRUSH = GetSysColorBrush(COLOR_WINDOW);
				LRESULT(brush.0 as isize)
			}
			WM_COMMAND => {
				if (wparam.0 & 0xffff) as i32 == ID_OK {
					let _ = DestroyWindow(hwnd);
					return LRESULT(0);
				}
				DefWindowProcW(hwnd, message, wparam, lparam)
			}
			WM_CLOSE => {
				let _ = DestroyWindow(hwnd);
				LRESULT(0)
			}
			WM_DESTROY => {
				if let Ok(mut current) = state().lock() {
					current.hwnd = 0;
					for font in current.fonts.drain(..) {
						let _ = DeleteObject(HGDIOBJ(font as *mut c_void));
					}
				}
				LRESULT(0)
			}
			_ => DefWindowProcW(hwnd, message, wparam, lparam),
		}
	}
}

fn wide(value: &str) -> Vec<u16> {
	value.encode_utf16().chain(std::iter::once(0)).collect()
}