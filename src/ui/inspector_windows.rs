// =======================================================
// src/ui/inspector_windows.rs — Win32 Inspector window
// =======================================================

use super::inspector_content::{Document, InspectorSnapshot, TAB_TITLES};
use crate::emulator::Result;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
	cell::Cell,
	ptr::{null, null_mut},
	sync::OnceLock,
};
use windows_sys::Win32::{
	Foundation::*,
	Graphics::Gdi::*,
	System::LibraryLoader::{GetModuleHandleW, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
	UI::{Controls::*, HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
};

/* Win32 owns the tabs, read-only edit controls and their scrolling. The boxed
 * state remains stable until the native window and all callbacks are destroyed.
 * Cell permits re-entrant window messages without aliased mutable references. */
use windows::Win32::UI::Controls::RichEdit as rich;

pub(super) struct NativeInspector {
	state: Box<State>,
	library: HMODULE,
	previous: [String; 3],
}
struct State {
	window: HWND,
	status: HWND,
	tabs: HWND,
	views: [HWND; 3],
	font: Cell<HFONT>,
	closed: Cell<bool>,
}
fn wide(text: &str) -> Vec<u16> {
	text.encode_utf16().chain(Some(0)).collect()
}

impl NativeInspector {
	pub fn new(parent: &winit::window::Window) -> Result<Self> {
		let RawWindowHandle::Win32(parent) = parent.window_handle()?.as_raw() else {
			return Err("Missing Win32 window".into());
		};
		/* Handles are created and destroyed on the host event-loop thread. All
		 * strings and message buffers live through their synchronous API calls. */
		unsafe {
			let instance = GetModuleHandleW(null());
			let class = wide("Breadbin466Inspector");
			static REGISTERED: OnceLock<bool> = OnceLock::new();
			if !*REGISTERED.get_or_init(|| {
				let wc = WNDCLASSW {
					lpfnWndProc: Some(procedure),
					hInstance: instance,
					lpszClassName: class.as_ptr(),
					hCursor: LoadCursorW(null_mut(), IDC_ARROW),
					hbrBackground: GetSysColorBrush(COLOR_WINDOW),
					..Default::default()
				};
				RegisterClassW(&wc) != 0
			}) {
				return Err(std::io::Error::last_os_error().into());
			}
			InitCommonControlsEx(&INITCOMMONCONTROLSEX {
				dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
				dwICC: ICC_TAB_CLASSES,
			});
			let owner = parent.hwnd.get() as HWND;
			let library = LoadLibraryExW(
				wide("Msftedit.dll").as_ptr(),
				null_mut(),
				LOAD_LIBRARY_SEARCH_SYSTEM32,
			);
			if library.is_null() {
				return Err(std::io::Error::last_os_error().into());
			}
			let dpi = GetDpiForWindow(owner).max(96) as i32;
			let window = CreateWindowExW(
				0,
				class.as_ptr(),
				wide("Breadbin466 — Inspector").as_ptr(),
				WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
				CW_USEDEFAULT,
				CW_USEDEFAULT,
				620 * dpi / 96,
				680 * dpi / 96,
				owner,
				null_mut(),
				instance,
				null(),
			);
			if window.is_null() {
				FreeLibrary(library);
				return Err(std::io::Error::last_os_error().into());
			}
			let child = |class: &str, style| {
				CreateWindowExW(
					0,
					wide(class).as_ptr(),
					wide("").as_ptr(),
					WS_CHILD | style,
					0,
					0,
					1,
					1,
					window,
					null_mut(),
					instance,
					null(),
				)
			};
			let status = child("STATIC", WS_VISIBLE);
			let tabs = child("SysTabControl32", WS_VISIBLE | WS_TABSTOP);
			let views = std::array::from_fn(|_| {
				child(
					"RICHEDIT50W",
					WS_VSCROLL
						| WS_TABSTOP | ES_MULTILINE as u32
						| ES_READONLY as u32
						| ES_AUTOVSCROLL as u32,
				)
			});
			if status.is_null() || tabs.is_null() || views.iter().any(|v| v.is_null()) {
				DestroyWindow(window);
				FreeLibrary(library);
				return Err("Cannot create Inspector controls".into());
			}
			for (index, title) in TAB_TITLES.iter().enumerate() {
				let mut title = wide(title);
				let item = TCITEMW {
					mask: TCIF_TEXT,
					pszText: title.as_mut_ptr(),
					..Default::default()
				};
				SendMessageW(tabs, TCM_INSERTITEMW, index, &item as *const _ as isize);
			}
			let state = Box::new(State {
				window,
				status,
				tabs,
				views,
				font: Cell::new(null_mut()),
				closed: Cell::new(false),
			});
			SetWindowLongPtrW(window, GWLP_USERDATA, &*state as *const State as isize);
			state.update_font();
			state.layout();
			ShowWindow(views[0], SW_SHOW);
			ShowWindow(window, SW_SHOW);
			Ok(Self {
				state,
				library,
				previous: std::array::from_fn(|_| String::new()),
			})
		}
	}
	pub fn show(&self) {
		unsafe {
			ShowWindow(self.state.window, SW_RESTORE);
			SetForegroundWindow(self.state.window);
		}
	}
	pub fn is_open(&self) -> bool {
		!self.state.closed.get()
	}
	pub fn is_visible(&self) -> bool {
		self.is_open()
			&& unsafe {
				IsWindowVisible(self.state.window) != 0 && IsIconic(self.state.window) == 0
			}
	}
	pub fn update(&mut self, snapshot: &InspectorSnapshot) {
		unsafe {
			SetWindowTextW(self.state.status, wide(&snapshot.status).as_ptr());
			for (index, document) in snapshot.documents().iter().enumerate() {
				let view = self.state.views[index];
				let (mut start, mut end) = (0u32, 0u32);
				SendMessageW(
					view,
					EM_GETSEL,
					&mut start as *mut _ as usize,
					&mut end as *mut _ as isize,
				);
				if self.previous[index] == document.text || start != end {
					continue;
				}
				let mut position = POINT::default();
				SendMessageW(
					view,
					rich::EM_GETSCROLLPOS,
					0,
					&mut position as *mut _ as isize,
				);
				let rtf = rich_text(document);
				let mut cursor = std::io::Cursor::new(rtf.as_bytes());
				let mut stream = rich::EDITSTREAM {
					dwCookie: &mut cursor as *mut _ as usize,
					dwError: 0,
					pfnCallback: Some(read_stream),
				};
				SendMessageW(
					view,
					rich::EM_STREAMIN,
					rich::SF_RTF as usize,
					&mut stream as *mut _ as isize,
				);
				SendMessageW(
					view,
					rich::EM_SETSCROLLPOS,
					0,
					&position as *const _ as isize,
				);
				self.previous[index].clone_from(&document.text);
			}
		}
	}
}
impl State {
	fn layout(&self) {
		unsafe {
			let mut r = RECT::default();
			GetClientRect(self.window, &mut r);
			let d = GetDpiForWindow(self.window).max(96) as i32;
			let m = 16 * d / 96;
			MoveWindow(self.status, m, m, (r.right - 2 * m).max(1), 24 * d / 96, 1);
			let top = 48 * d / 96;
			MoveWindow(
				self.tabs,
				m,
				top,
				(r.right - 2 * m).max(1),
				(r.bottom - top - m).max(1),
				1,
			);
			let mut page = RECT::default();
			GetClientRect(self.tabs, &mut page);
			SendMessageW(self.tabs, TCM_ADJUSTRECT, 0, &mut page as *mut _ as isize);
			for view in self.views {
				MoveWindow(
					view,
					m + page.left + m / 2,
					top + page.top + m / 2,
					(page.right - page.left - m).max(1),
					(page.bottom - page.top - m).max(1),
					1,
				);
			}
		}
	}
	fn update_font(&self) {
		unsafe {
			let dpi = GetDpiForWindow(self.window).max(96) as i32;
			let font = CreateFontW(
				-13 * dpi / 96,
				0,
				0,
				0,
				FW_NORMAL as i32,
				0,
				0,
				0,
				DEFAULT_CHARSET as u32,
				OUT_DEFAULT_PRECIS as u32,
				CLIP_DEFAULT_PRECIS as u32,
				CLEARTYPE_QUALITY as u32,
				0,
				wide("Segoe UI").as_ptr(),
			);
			if font.is_null() {
				return;
			}
			for control in [self.status, self.tabs].into_iter().chain(self.views) {
				SendMessageW(control, WM_SETFONT, font as usize, 1);
			}
			let old = self.font.replace(font);
			if !old.is_null() {
				DeleteObject(old);
			}
		}
	}
}
/* User data is only borrowed during synchronous messages. WM_DESTROY marks
 * the handle dead before the owning Rust object can release its state. */
unsafe extern "system" fn procedure(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
	unsafe {
		let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const State;
		if let Some(state) = pointer.as_ref() {
			match message {
				WM_SIZE => {
					state.layout();
					return 0;
				}
				WM_NOTIFY => {
					let notification = &*(l as *const NMHDR);
					if notification.hwndFrom == state.tabs && notification.code == TCN_SELCHANGE {
						let selected = SendMessageW(state.tabs, TCM_GETCURSEL, 0, 0) as usize;
						for (index, view) in state.views.iter().enumerate() {
							ShowWindow(*view, if index == selected { SW_SHOW } else { SW_HIDE });
						}
						return 0;
					}
				}
				WM_DPICHANGED => {
					let r = &*(l as *const RECT);
					SetWindowPos(
						window,
						null_mut(),
						r.left,
						r.top,
						r.right - r.left,
						r.bottom - r.top,
						SWP_NOZORDER | SWP_NOACTIVATE,
					);
					state.update_font();
					state.layout();
					return 0;
				}
				WM_GETMINMAXINFO => {
					let info = &mut *(l as *mut MINMAXINFO);
					let dpi = GetDpiForWindow(window).max(96) as i32;
					info.ptMinTrackSize = POINT {
						x: 440 * dpi / 96,
						y: 340 * dpi / 96,
					};
					return 0;
				}
				WM_CLOSE => {
					DestroyWindow(window);
					return 0;
				}
				WM_DESTROY => {
					state.closed.set(true);
					SetWindowLongPtrW(window, GWLP_USERDATA, 0);
					return 0;
				}
				_ => {}
			}
		}
		DefWindowProcW(window, message, w, l)
	}
}
impl Drop for NativeInspector {
	fn drop(&mut self) {
		unsafe {
			if self.is_open() {
				DestroyWindow(self.state.window);
			}
			FreeLibrary(self.library);
			let font = self.state.font.get();
			if !font.is_null() {
				DeleteObject(font);
			}
		}
	}
}

/* RTF carries native paragraph and heading formatting. UTF-16 escapes retain
 * arbitrary media paths without allowing their contents to become RTF syntax. */
fn rich_text(document: &Document) -> String {
	use std::fmt::Write;
	let mut output = String::from(r"{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\uc1 ");
	let mut offset = 0;
	for line in document.text.split_inclusive('\n') {
		let heading = document.headings.iter().any(|range| range.start == offset);
		output.push_str(if heading {
			r"\pard\sb160\sa120\b\fs24 "
		} else {
			r"\pard\li2100\fi-2100\tx2100\sa100\b0\fs20 "
		});
		for unit in line.trim_end_matches('\n').encode_utf16() {
			match unit {
				9 => output.push_str(r"\tab "),
				92 | 123 | 125 => {
					output.push('\\');
					output.push(char::from_u32(unit as u32).unwrap());
				}
				32..=126 => output.push(char::from_u32(unit as u32).unwrap()),
				_ => {
					let _ = write!(output, r"\u{}?", unit as i16);
				}
			}
		}
		output.push_str(r"\par ");
		offset += line.len();
	}
	output.push('}');
	output
}
/* RichEdit calls this synchronously with its writable byte buffer. The cursor
 * remains on the caller's stack until EM_STREAMIN has completely returned. */
unsafe extern "system" fn read_stream(
	cookie: usize,
	buffer: *mut u8,
	length: i32,
	written: *mut i32,
) -> u32 {
	use std::io::Read;
	unsafe {
		if length < 0 {
			return 1;
		}
		let cursor = &mut *(cookie as *mut std::io::Cursor<&[u8]>);
		match cursor.read(std::slice::from_raw_parts_mut(buffer, length as usize)) {
			Ok(count) => {
				*written = count as i32;
				0
			}
			Err(_) => {
				*written = 0;
				1
			}
		}
	}
}