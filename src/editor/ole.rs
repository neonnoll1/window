//! Rich Edit hosts text and pictures but does not host executable or linked OLE objects.
use std::{ffi::c_void, ptr};
use windows_sys::Win32::Foundation::{E_ACCESSDENIED, E_NOINTERFACE, E_NOTIMPL, E_POINTER, S_OK};
use windows_sys::core::{GUID, HRESULT};

type Object = *mut c_void;

#[repr(C)]
struct CallbackVtable {
    query_interface: unsafe extern "system" fn(Object, *const GUID, *mut Object) -> HRESULT,
    add_ref: extern "system" fn(Object) -> u32,
    release: extern "system" fn(Object) -> u32,
    new_storage: unsafe extern "system" fn(Object, *mut Object) -> HRESULT,
    in_place_context: extern "system" fn(Object, *mut Object, *mut Object, Object) -> HRESULT,
    show_container: extern "system" fn(Object, i32) -> HRESULT,
    query_insert: extern "system" fn(Object, *const GUID, Object, i32) -> HRESULT,
    delete_object: extern "system" fn(Object, Object) -> HRESULT,
    query_data: extern "system" fn(Object, Object, *mut u16, u32, i32, Object) -> HRESULT,
    context_help: extern "system" fn(Object, i32) -> HRESULT,
    clipboard_data: extern "system" fn(Object, Object, u32, *mut Object) -> HRESULT,
    drag_effect: extern "system" fn(Object, i32, u32, *mut u32) -> HRESULT,
    context_menu: extern "system" fn(Object, u16, Object, Object, *mut Object) -> HRESULT,
}

#[repr(C)]
struct Callback {
    vtable: &'static CallbackVtable,
}

static CALLBACK: Callback = Callback {
    vtable: &CallbackVtable {
        query_interface,
        add_ref: static_ref,
        release: static_ref,
        new_storage,
        in_place_context,
        show_container: no_ui,
        query_insert,
        delete_object,
        query_data,
        context_help: no_ui,
        clipboard_data,
        drag_effect,
        context_menu,
    },
};

pub fn callback() -> isize {
    &CALLBACK as *const _ as isize
}

unsafe extern "system" fn query_interface(
    this: Object,
    id: *const GUID,
    result: *mut Object,
) -> HRESULT {
    if result.is_null() {
        return E_POINTER;
    }
    unsafe {
        result.write(ptr::null_mut());
    }
    let Some(id) = (unsafe { id.as_ref() }) else {
        return E_POINTER;
    };
    let same_interface = matches!(id.data1, 0 | 0x0002_0d03)
        && id.data2 == 0
        && id.data3 == 0
        && id.data4 == [0xc0, 0, 0, 0, 0, 0, 0, 0x46];
    if !same_interface {
        return E_NOINTERFACE;
    }
    unsafe {
        result.write(this);
    }
    static_ref(this);
    S_OK
}

// The singleton is valid until the process exits, regardless of the control's references.
extern "system" fn static_ref(_: Object) -> u32 {
    1
}

unsafe extern "system" fn new_storage(_: Object, storage: *mut Object) -> HRESULT {
    if !storage.is_null() {
        unsafe {
            storage.write(ptr::null_mut());
        }
    }
    E_ACCESSDENIED
}

extern "system" fn query_insert(_: Object, _: *const GUID, _: Object, _: i32) -> HRESULT {
    E_ACCESSDENIED
}
extern "system" fn delete_object(_: Object, _: Object) -> HRESULT {
    S_OK
}
extern "system" fn query_data(
    _: Object,
    _: Object,
    _: *mut u16,
    _: u32,
    _: i32,
    _: Object,
) -> HRESULT {
    S_OK
}

extern "system" fn no_ui(_: Object, _: i32) -> HRESULT {
    E_NOTIMPL
}
extern "system" fn in_place_context(
    _: Object,
    _: *mut Object,
    _: *mut Object,
    _: Object,
) -> HRESULT {
    E_NOTIMPL
}
extern "system" fn clipboard_data(_: Object, _: Object, _: u32, _: *mut Object) -> HRESULT {
    E_NOTIMPL
}
extern "system" fn drag_effect(_: Object, _: i32, _: u32, _: *mut u32) -> HRESULT {
    E_NOTIMPL
}
extern "system" fn context_menu(
    _: Object,
    _: u16,
    _: Object,
    _: Object,
    _: *mut Object,
) -> HRESULT {
    E_NOTIMPL
}
