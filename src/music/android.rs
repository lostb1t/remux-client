//! The native component "remux.music" on Android: the music player (`MusicPlayer.java`), which
//! is one for the app and lives on when the component is gone. The component is an empty view
//! in the shell; its commands go to the player over JNI, and how the music goes comes back as the
//! event `state`, read four times a second on a thread of its own (as for the video).

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nsh::jni::JNIEnv;
use nsh::jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use nsh::{ComponentContext, Components, NativeComponent};
use serde_json::Value as JsonValue;

/// The name the shell asks for.
pub const NAME: &str = "remux.music";
const CLASS: &str = "com/github/remux/nsh/MusicPlayer";

struct Music {
    view: GlobalRef,
    /// The class, found once on the UI thread: a thread of Rust's own finds only the system's
    /// classes, not the app's.
    class: GlobalRef,
    alive: Arc<AtomicBool>,
}

/// Registers the component.
pub fn register(components: &Components) {
    components.register(NAME, Rc::new(|context: ComponentContext| -> Box<dyn NativeComponent> { Box::new(Music::new(context)) }));
}

impl Music {
    fn new(context: ComponentContext) -> Self {
        let (view, class) = context
            .with_env(|env| {
                let android = context.android_context();
                let class = env.find_class(CLASS)?;
                env.call_static_method(&class, "init", "(Landroid/content/Context;)V", &[JValue::Object(android.as_obj())])?;
                // What the screen does with HDR, for the device profile: asked here, once, as
                // this is where the app has the context (the component is made with the shell).
                if let Ok(info) = env.find_class("com/github/remux/nsh/DeviceInfo") {
                    if let Ok(bits) = env.call_static_method(&info, "hdrBits", "(Landroid/content/Context;)I", &[JValue::Object(android.as_obj())]).and_then(|v| v.i()) {
                        crate::capabilities::set_display_hdr(bits);
                    }
                    // Finding Chromecasts (a phone; `multicastLock` leaves a TV out).
                    if env.call_static_method(&info, "multicastLock", "(Landroid/content/Context;)V", &[JValue::Object(android.as_obj())]).is_err() {
                        let _ = env.exception_clear();
                    }
                }
                let view = env.new_object("android/view/View", "(Landroid/content/Context;)V", &[JValue::Object(android.as_obj())])?;
                Ok((env.new_global_ref(view)?, env.new_global_ref(class)?))
            })
            .expect("the MusicPlayer class is in the app (android/out/classes.dex)");
        let alive = Arc::new(AtomicBool::new(true));
        {
            let (alive, context, class) = (alive.clone(), context.clone(), class.clone());
            std::thread::spawn(move || {
                let mut last = String::new();
                while alive.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(250));
                    let snapshot = context.with_env(|env| {
                        let state = env.call_static_method(<&JClass>::from(class.as_obj()), "snapshot", "()Ljava/lang/String;", &[])?.l()?;
                        let state: String = env.get_string(&JString::from(state))?.into();
                        Ok(state)
                    });
                    if let Ok(state) = snapshot {
                        if state != last {
                            if let Ok(json) = serde_json::from_str::<JsonValue>(&state) {
                                context.emit("state", json);
                            }
                            last = state;
                        }
                    }
                }
            });
        }
        Self { view, class, alive }
    }
}

/// Calls the player.
fn call(env: &mut JNIEnv<'_>, class: &GlobalRef, method: &str, signature: &str, args: &[JValue<'_, '_>]) {
    if let Err(error) = env.call_static_method(<&JClass>::from(class.as_obj()), method, signature, args) {
        log::error!("MusicPlayer.{method}: {error}");
        let _ = env.exception_clear();
    }
}

impl NativeComponent for Music {
    fn view(&self) -> &GlobalRef {
        &self.view
    }

    fn set_prop(&mut self, _env: &mut JNIEnv<'_>, _name: &str, _value: Option<&JsonValue>) {}

    fn command(&mut self, env: &mut JNIEnv<'_>, name: &str, args: &JsonValue) {
        let int = |key: &str| args.get(key).and_then(JsonValue::as_i64).unwrap_or(0) as i32;
        let float = |key: &str| args.get(key).and_then(JsonValue::as_f64).unwrap_or(0.0);
        match name {
            "setQueue" | "replaceQueue" => {
                let tracks = args.get("tracks").map(JsonValue::to_string).unwrap_or_else(|| "[]".into());
                let Ok(tracks) = env.new_string(tracks) else { return };
                let tracks = JObject::from(tracks);
                if name == "setQueue" {
                    let play = args.get("play").and_then(JsonValue::as_bool).unwrap_or(true);
                    call(env, &self.class, "setQueue", "(Ljava/lang/String;IZ)V", &[JValue::Object(&tracks), JValue::Int(int("index")), JValue::Bool(play.into())]);
                } else {
                    call(env, &self.class, "replaceQueue", "(Ljava/lang/String;I)V", &[JValue::Object(&tracks), JValue::Int(int("index"))]);
                }
            }
            "play" => call(env, &self.class, "play", "()V", &[]),
            "pause" => call(env, &self.class, "pause", "()V", &[]),
            "close" => call(env, &self.class, "close", "()V", &[]),
            "seek" => call(env, &self.class, "seek", "(D)V", &[JValue::Double(float("ms"))]),
            "skipTo" => call(env, &self.class, "skipTo", "(I)V", &[JValue::Int(int("index"))]),
            "repeat" => call(env, &self.class, "repeat", "(I)V", &[JValue::Int(int("mode"))]),
            "volume" => call(env, &self.class, "volume", "(D)V", &[JValue::Double(float("volume"))]),
            _ => {}
        }
    }

    fn destroy(&mut self, _env: &mut JNIEnv<'_>) {
        // The player plays on; only the reading stops.
        self.alive.store(false, Ordering::Relaxed);
    }
}
