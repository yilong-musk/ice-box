// SPDX-License-Identifier: GPL-3.0-or-later

//! Install the OS trust store before any HTTPS subscription fetch.
//! Called from `MainActivity` after the Tauri runtime has loaded this library.

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject, JString};
use jni::EnvUnowned;

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yilongmusk_icebox_Native_initVerifier<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let message = match rustls_platform_verifier::android::init_with_env(env, context) {
                Ok(()) => "verifier ready".to_string(),
                Err(err) => format!("verifier init failed: {err}"),
            };
            log::info!("{message}");
            env.new_string(&message)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
