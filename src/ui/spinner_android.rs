//! The native component "remux.spinner" on Android: the system's own spinner (`Spinner.java`),
//! which NSH has no element for.

use std::rc::Rc;

use nsh::jni::JNIEnv;
use nsh::jni::objects::{GlobalRef, JValue};
use nsh::{ComponentContext, Components, NativeComponent};
use serde_json::Value as JsonValue;

pub const NAME: &str = "remux.spinner";

struct Spinner {
    view: GlobalRef,
}

pub fn register(components: &Components) {
    components.register(
        NAME,
        Rc::new(|context: ComponentContext| -> Box<dyn NativeComponent> {
            let view = context
                .with_env(|env| {
                    let bar = env
                        .call_static_method(
                            "com/github/remux/nsh/Spinner",
                            "make",
                            "(Landroid/content/Context;)Landroid/widget/ProgressBar;",
                            &[JValue::Object(context.android_context().as_obj())],
                        )?
                        .l()?;
                    env.new_global_ref(bar)
                })
                .expect("the Spinner class is in the app (android/out/classes.dex)");
            Box::new(Spinner { view })
        }),
    );
}

impl NativeComponent for Spinner {
    fn view(&self) -> &GlobalRef {
        &self.view
    }

    fn set_prop(&mut self, _env: &mut JNIEnv<'_>, _name: &str, _value: Option<&JsonValue>) {}

    fn command(&mut self, _env: &mut JNIEnv<'_>, _name: &str, _args: &JsonValue) {}

    fn destroy(&mut self, _env: &mut JNIEnv<'_>) {}
}
