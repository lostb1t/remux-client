//! The native component "remux.exo" on Android: ExoPlayer, in the Java view `ExoView`
//! (`android/src`), placed by NSH like any view. Its props and commands go to the view over JNI;
//! how playback goes comes back as the event `state`, read from the view's snapshot four times a
//! second (as Remux polls its player), on a thread of its own.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nsh::jni::JNIEnv;
use nsh::jni::objects::{GlobalRef, JObject, JString, JValue};
use nsh::{ComponentContext, Components, NativeComponent};
use serde_json::Value as JsonValue;

/// The name the app's player asks for.
pub const NAME: &str = "remux.exo";

struct Exo {
    view: GlobalRef,
    alive: Arc<AtomicBool>,
}

/// Registers the component.
pub fn register(components: &Components) {
    components.register(NAME, Rc::new(|context: ComponentContext| -> Box<dyn NativeComponent> { Box::new(Exo::new(context)) }));
}

impl Exo {
    fn new(context: ComponentContext) -> Self {
        let view = context
            .with_env(|env| {
                let view = env.new_object(
                    "com/github/remux/nsh/ExoView",
                    "(Landroid/content/Context;)V",
                    &[JValue::Object(context.android_context().as_obj())],
                )?;
                env.new_global_ref(view)
            })
            .expect("the ExoView class is in the app (android/out/classes.dex)");
        let alive = Arc::new(AtomicBool::new(true));
        // How playback goes, to the app, until the element is gone.
        {
            let (view, alive, context) = (view.clone(), alive.clone(), context.clone());
            std::thread::spawn(move || {
                let mut last = String::new();
                while alive.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(250));
                    let snapshot = context.with_env(|env| {
                        let state = env.call_method(view.as_obj(), "snapshot", "()Ljava/lang/String;", &[])?.l()?;
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
        Self { view, alive }
    }

    fn call(&self, env: &mut JNIEnv<'_>, method: &str, signature: &str, args: &[JValue<'_, '_>]) {
        if let Err(error) = env.call_method(self.view.as_obj(), method, signature, args) {
            log::error!("ExoView.{method}: {error}");
            let _ = env.exception_clear();
        }
    }
}

impl NativeComponent for Exo {
    fn view(&self) -> &GlobalRef {
        &self.view
    }

    fn set_prop(&mut self, env: &mut JNIEnv<'_>, name: &str, value: Option<&JsonValue>) {
        match name {
            // What to play (see `ExoView.load`).
            "source" => {
                let Some(source) = value.filter(|value| value.is_object()) else { return };
                let Ok(json) = env.new_string(source.to_string()) else { return };
                let json = JObject::from(json);
                self.call(env, "load", "(Ljava/lang/String;)V", &[JValue::Object(&json)]);
            }
            "paused" => {
                let paused = value.and_then(JsonValue::as_bool).unwrap_or(false);
                self.call(env, "setPaused", "(Z)V", &[JValue::Bool(paused.into())]);
            }
            "subtitle" => {
                let index = value.and_then(JsonValue::as_i64).unwrap_or(-1);
                self.call(env, "subtitle", "(J)V", &[JValue::Long(index)]);
            }
            "audio" => {
                let index = value.and_then(JsonValue::as_i64).unwrap_or(-1);
                self.call(env, "audio", "(J)V", &[JValue::Long(index)]);
            }
            _ => {}
        }
    }

    fn command(&mut self, env: &mut JNIEnv<'_>, name: &str, args: &JsonValue) {
        if name == "seek" {
            let ms = args.get("ms").and_then(JsonValue::as_f64).unwrap_or(0.0);
            self.call(env, "seek", "(D)V", &[JValue::Double(ms)]);
        }
    }

    fn destroy(&mut self, env: &mut JNIEnv<'_>) {
        self.alive.store(false, Ordering::Relaxed);
        self.call(env, "release", "()V", &[]);
    }
}
