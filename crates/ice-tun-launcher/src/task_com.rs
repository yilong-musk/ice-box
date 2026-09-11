// SPDX-License-Identifier: GPL-3.0-or-later

//! Register `ice-box-tun` from an in-memory XML string (no on-disk task file).

use windows::core::BSTR;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::TaskScheduler::{
    ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN,
};
use windows::Win32::System::Variant::VARIANT;

/// Import `xml` as `\{TUN_TASK_NAME}`. `xml` must already be COM-safe
/// ([`ice_tun_pin::task_xml_for_com_bstr`]).
pub fn register_task_xml(xml: &str) -> Result<(), String> {
    register_task_xml_inner(xml).map_err(|err| format!("ITaskService::RegisterTask failed: {err}"))
}

fn register_task_xml_inner(xml: &str) -> windows::core::Result<()> {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let uninit = hr.is_ok();
        let result = (|| {
            let service: ITaskService = CoCreateInstance(
                &TaskScheduler,
                None,
                CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER,
            )?;
            let empty = VARIANT::default();
            service.Connect(&empty, &empty, &empty, &empty)?;
            let folder = service.GetFolder(&BSTR::from("\\"))?;
            let _task = folder.RegisterTask(
                &BSTR::from(ice_tun_pin::TUN_TASK_NAME),
                &BSTR::from(xml),
                TASK_CREATE_OR_UPDATE.0,
                &empty,
                &empty,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &empty,
            )?;
            Ok(())
        })();
        if uninit {
            CoUninitialize();
        }
        result
    }
}
