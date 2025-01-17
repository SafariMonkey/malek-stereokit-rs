use crate::model::Model;
use derive_builder::Builder;
use once_cell::unsync::OnceCell;
use std::cell::{Ref, RefCell};
use std::ffi::{c_void, CString};
use std::fmt::Error;
use std::marker::PhantomData;
use std::os::unix::thread;
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::rc::{Rc, Weak};
use std::sync::Mutex;
use std::{mem, ptr};
use stereokit_sys::{
	assets_releaseref_threadsafe, bool32_t, color32, depth_mode_, display_blend_, display_mode_,
	log_, material_t, model_t, sk_settings_t,
};

pub enum DisplayMode {
	MixedReality = 0,
	Flatscreen = 1,
	None = 2,
}
pub enum DisplayBlend {
	None = 0,
	Opaque = 1,
	Additive = 2,
	Blend = 4,
	AnyTransparent = 6,
}
pub enum DepthMode {
	Balanced = 0,
	D16 = 1,
	D32 = 2,
	Stencil = 3,
}
pub enum LogFilter {
	None = 0,
	Diagnostic = 1,
	Inform = 2,
	Warning = 3,
	Error = 4,
}

thread_local! {
	static GLOBAL_STATE: RefCell<bool> = RefCell::new(false);
}

#[derive(Builder)]
#[builder(name = "Settings", pattern = "owned", setter(into), build_fn(skip))]
pub struct SKSettingsBuilt {
	app_name: String,
	assets_folder: String,
	display_preference: DisplayMode,
	blend_preference: DisplayBlend,
	no_flatscreen_fallback: bool,
	depth_mode: DepthMode,
	log_filter: LogFilter,
	overlay_app: bool,
	overlay_priority: u32,
	flatscreen_pos_x: u32,
	flatscreen_pos_y: u32,
	flatscreen_width: u32,
	flatscreen_height: u32,
	disable_flatscreen_mr_sim: bool,
	disable_unfocused_sleep: bool,
}

impl Settings {
	pub fn init(self) -> Option<StereoKit> {
		if GLOBAL_STATE.with(|f| *f.borrow()) {
			return None;
		}

		let c_settings = sk_settings_t {
			app_name: CString::new(
				self.app_name
					.unwrap_or_else(|| "sk_app".to_owned())
					.as_str(),
			)
			.unwrap()
			.into_raw(),
			assets_folder: CString::new(self.assets_folder.unwrap_or_default().as_str())
				.unwrap()
				.into_raw(),
			display_preference: self.display_preference.unwrap_or(DisplayMode::MixedReality)
				as display_mode_,
			blend_preference: self.blend_preference.unwrap_or(DisplayBlend::None) as display_blend_,
			no_flatscreen_fallback: self.no_flatscreen_fallback.unwrap_or_default() as bool32_t,
			depth_mode: self.depth_mode.unwrap_or(DepthMode::Balanced) as depth_mode_,
			log_filter: self.log_filter.unwrap_or(LogFilter::Warning) as log_,
			overlay_app: self.overlay_app.unwrap_or_default() as bool32_t,
			overlay_priority: self.overlay_priority.unwrap_or_default(),
			flatscreen_pos_x: self.flatscreen_pos_x.unwrap_or_default() as i32,
			flatscreen_pos_y: self.flatscreen_pos_y.unwrap_or_default() as i32,
			flatscreen_width: self.flatscreen_width.unwrap_or_default() as i32,
			flatscreen_height: self.flatscreen_height.unwrap_or_default() as i32,
			disable_flatscreen_mr_sim: self.disable_flatscreen_mr_sim.unwrap_or_default()
				as bool32_t,
			disable_unfocused_sleep: self.disable_unfocused_sleep.unwrap_or_default() as bool32_t,
			android_java_vm: ptr::null_mut(),
			android_activity: ptr::null_mut(),
		};
		unsafe {
			if stereokit_sys::sk_init(c_settings) != 0 {
				GLOBAL_STATE.with(|f| *f.borrow_mut() = true);
				Some(StereoKit {
					ran: OnceCell::new(),
					handle: Rc::new(StereoKitInstance),
					lifetime_constraint: PhantomData,
				})
			} else {
				None
			}
		}
	}
}

pub(crate) struct StereoKitInstance;

#[derive(Clone)]
pub(crate) struct StereoKitInstanceWrapper(Weak<StereoKitInstance>);

impl StereoKitInstanceWrapper {
	pub(crate) fn valid(&self) -> Result<(), Error> {
		self.0.upgrade().map(|_| ()).ok_or(Error)
	}
}

pub struct StereoKit {
	ran: OnceCell<()>,
	handle: Rc<StereoKitInstance>,
	lifetime_constraint: PhantomData<*const ()>,
}
pub struct DrawContext(PhantomData<*const ()>);

unsafe extern "C" fn private_update_fn(context: *mut c_void) {
	let func_ptr: *mut &mut dyn FnMut(&DrawContext) = context.cast();
	(*func_ptr)(&DrawContext(PhantomData));
}
unsafe extern "C" fn private_shutdown_fn(context: *mut c_void) {
	let func_ptr: *mut &mut dyn FnMut() = context.cast();
	(*func_ptr)();

	GLOBAL_STATE.with(|f| *f.borrow_mut() = false);
}

impl StereoKit {
	pub fn run(&self, mut on_update: impl FnMut(&DrawContext), mut on_close: impl FnMut()) {
		if self.ran.set(()).is_err() {
			return;
		}
		let mut dyn_update: &mut dyn FnMut(&DrawContext) = &mut on_update;
		let mut dyn_close: &mut dyn FnMut() = &mut on_close;

		let ptr_update: *mut &mut dyn FnMut(&DrawContext) = &mut dyn_update;
		let ptr_close: *mut &mut dyn FnMut() = &mut dyn_close;

		unsafe {
			stereokit_sys::sk_run_data(
				Some(private_update_fn),
				ptr_update as *mut c_void,
				Some(private_shutdown_fn),
				ptr_close as *mut c_void,
			);
		}
	}

	pub fn quit(&self) {
		unsafe { stereokit_sys::sk_quit() };
	}

	pub(crate) fn get_wrapper(&self) -> StereoKitInstanceWrapper {
		StereoKitInstanceWrapper(Rc::downgrade(&self.handle))
	}

	pub fn time_elapsed(&self) -> f64 {
		unsafe { stereokit_sys::time_elapsed() }
	}
	pub fn time_elapsedf(&self) -> f32 {
		unsafe { stereokit_sys::time_elapsedf() }
	}
	pub fn time_getf_unscaled(&self) -> f32 {
		unsafe { stereokit_sys::time_getf_unscaled() }
	}
	pub fn time_elapsed_unscaled(&self) -> f64 {
		unsafe { stereokit_sys::time_elapsed_unscaled() }
	}
	pub fn time_elapsedf_unscaled(&self) -> f32 {
		unsafe { stereokit_sys::time_elapsedf_unscaled() }
	}
	pub fn time_get(&self) -> f64 {
		unsafe { stereokit_sys::time_get() }
	}
	pub fn time_getf(&self) -> f32 {
		unsafe { stereokit_sys::time_getf() }
	}
	pub fn time_get_unscaled(&self) -> f64 {
		unsafe { stereokit_sys::time_get_unscaled() }
	}
}

impl Drop for StereoKit {
	fn drop(&mut self) {
		if self.ran.get().is_none() {
			unsafe { stereokit_sys::sk_shutdown() }
		};
	}
}

pub enum Asset {
	Model(Model),
	//Font(Font)
}
pub fn asset_releaseref(asset: Asset) {
	match asset {
		Asset::Model(mut model) => {
			let my_var = &mut model.model as *mut _ as *mut std::ffi::c_void;
			unsafe {
				assets_releaseref_threadsafe(my_var);
			}
		}
	}
}
