//! The smallest plugin that does something: it writes one line to `logs\w2r.log` when loaded.
//! Copy this folder to start a new plugin (see CONTRIBUTING.md).

use w2r_abi::*;

unsafe extern "C" fn on_load(host: *const W2rHost) -> W2rStatus {
    let text = "hello from hello-example";
    let msg = W2rStr {
        ptr: text.as_ptr().cast(),
        len: text.len() as u32,
    };
    unsafe { ((*host).log)((*host).token, W2R_LOG_INFO, msg) }
}

static PLUGIN: W2rPlugin = W2rPlugin {
    struct_size: W2R_PLUGIN_V1_SIZE,
    on_load: Some(on_load),
    on_event: None,
    on_ui: None,
    on_unload: None,
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn w2r_plugin_main(_host: *const W2rHost) -> *const W2rPlugin {
    &PLUGIN
}
