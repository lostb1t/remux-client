//! The parts of Shaka Player that playing HLS needs. The library itself is `js/shaka.js`.

use wasm_bindgen::prelude::*;
use web_sys::HtmlMediaElement;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["shaka", "polyfill"], js_name = installAll)]
    pub fn install_polyfills();

    #[wasm_bindgen(js_namespace = shaka, js_name = Player)]
    pub type Player;

    #[wasm_bindgen(constructor, js_namespace = shaka, js_class = "Player")]
    pub fn new() -> Player;

    #[wasm_bindgen(method, catch)]
    pub async fn attach(this: &Player, media: &HtmlMediaElement) -> Result<JsValue, JsValue>;

    /// `start_time` in seconds, or `undefined` to start at the beginning.
    #[wasm_bindgen(method, catch)]
    pub async fn load(this: &Player, uri: &str, start_time: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch)]
    pub async fn destroy(this: &Player) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, js_name = addEventListener)]
    pub fn add_event_listener(this: &Player, kind: &str, listener: &Closure<dyn FnMut(JsValue)>);
}

/// Whether this browser can run Shaka.
pub fn is_browser_supported() -> bool {
    let call = || -> Option<bool> {
        let shaka = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("shaka")).ok()?;
        let player = js_sys::Reflect::get(&shaka, &JsValue::from_str("Player")).ok()?;
        let check = js_sys::Reflect::get(&player, &JsValue::from_str("isBrowserSupported")).ok()?;
        check.dyn_ref::<js_sys::Function>()?.call0(&player).ok()?.as_bool()
    };
    call().unwrap_or(false)
}

/// Whether the Shaka script has loaded.
pub fn is_loaded() -> bool {
    js_sys::Reflect::has(&js_sys::global(), &JsValue::from_str("shaka")).unwrap_or(false)
}

#[wasm_bindgen(module = "/js/shaka.js")]
extern "C" {
    /// Shaka itself, a module of the crate (`js/shaka.js`): NSH's page loads scripts of its own
    /// origin only, so not from cdnjs as Remux does.
    #[wasm_bindgen(js_name = shakaReady)]
    fn shaka_ready() -> bool;
}

/// Whether Shaka is there (the module sets it up as the app loads).
pub async fn ensure_loaded() -> bool {
    shaka_ready() && is_loaded()
}
