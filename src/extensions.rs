use crate::chompfile::ChompTaskMaybeTemplatedNoDefault;
use crate::engines::BatchCmd;
use crate::engines::CmdOp;
use anyhow::{anyhow, Error, Result};
use serde_v8::from_v8;
use serde_v8::to_v8;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use v8;

pub struct ExtensionEnvironment {
    isolate: v8::OwnedIsolate,
    global_context: v8::Global<v8::Context>,
}

struct Extensions {
    pub tasks: Vec<ChompTaskMaybeTemplatedNoDefault>,
    templates: HashMap<String, v8::Global<v8::Function>>,
    batchers: Vec<(String, v8::Global<v8::Function>)>,
}

impl Extensions {
    fn new() -> Self {
        Extensions {
            tasks: Vec::new(),
            templates: HashMap::new(),
            batchers: Vec::new(),
        }
    }
}

pub fn init_js_platform() {
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
}

fn chomp_log(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _rv: v8::ReturnValue,
) {
    let mut msg = String::new();
    let len = args.length();
    let mut i = 0;
    while i < len {
        let arg = args.get(i).to_string(scope).unwrap();
        if i > 0 {
            msg.push_str(", ");
        }
        msg.push_str(&arg.to_rust_string_lossy(scope));
        i = i + 1;
    }
    println!("{}", &msg);
}

fn chomp_register_task(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _rv: v8::ReturnValue,
) {
    let task: ChompTaskMaybeTemplatedNoDefault = {
        let tc_scope = &mut v8::TryCatch::new(scope);
        from_v8(tc_scope, args.get(0)).expect("Unable to register task")
    };
    let mut extension_env = scope
    .get_slot::<Rc<RefCell<Extensions>>>()
    .unwrap()
    .borrow_mut();
    extension_env.tasks.push(task);
}

fn chomp_register_template(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _rv: v8::ReturnValue,
) {
    let name = args.get(0).to_string(scope).unwrap();
    let name_str = name.to_rust_string_lossy(scope);
    let tpl = v8::Local::<v8::Function>::try_from(args.get(1)).unwrap();
    let tpl_global = v8::Global::new(scope, tpl);

    let mut extension_env = scope
    .get_slot::<Rc<RefCell<Extensions>>>()
    .unwrap()
    .borrow_mut();
    extension_env.templates.insert(name_str, tpl_global);
}

fn chomp_register_batcher(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _rv: v8::ReturnValue,
) {
    let name = args.get(0).to_string(scope).unwrap();
    let name_str = name.to_rust_string_lossy(scope);
    let batch = v8::Local::<v8::Function>::try_from(args.get(1)).unwrap();
    let batch_global = v8::Global::new(scope, batch);

    let mut extension_env = scope
    .get_slot::<Rc<RefCell<Extensions>>>()
    .unwrap()
    .borrow_mut();
    extension_env.batchers.push((name_str, batch_global));
}

impl ExtensionEnvironment {
    pub fn new(global_env: &HashMap<String, String>) -> Self {
        let mut isolate = v8::Isolate::new(Default::default());

        let global_context = {
            let mut handle_scope = v8::HandleScope::new(&mut isolate);
            let context = v8::Context::new(&mut handle_scope);
            let global = context.global(&mut handle_scope);

            let scope = &mut v8::ContextScope::new(&mut handle_scope, context);

            let chomp_key = v8::String::new(scope, "Chomp").unwrap();
            let chomp_val = v8::Object::new(scope);
            global.set(scope, chomp_key.into(), chomp_val.into());

            let console_key = v8::String::new(scope, "console").unwrap();
            let console_val = v8::Object::new(scope);
            global.set(scope, console_key.into(), console_val.into());

            let log_fn = v8::FunctionTemplate::new(scope, chomp_log).get_function(scope).unwrap();
            let log_key = v8::String::new(scope, "log").unwrap();
            console_val.set(scope, log_key.into(), log_fn.into());

            let version_key = v8::String::new(scope, "version").unwrap();
            let version_str = v8::String::new(scope, "0.1").unwrap();
            chomp_val.set(scope, version_key.into(), version_str.into());

            let task_fn = v8::FunctionTemplate::new(scope, chomp_register_task).get_function(scope).unwrap();
            let task_key = v8::String::new(scope, "registerTask").unwrap();
            chomp_val.set(scope, task_key.into(), task_fn.into());

            let tpl_fn = v8::FunctionTemplate::new(scope, chomp_register_template).get_function(scope).unwrap();
            let template_key = v8::String::new(scope, "registerTemplate").unwrap();
            chomp_val.set(scope, template_key.into(), tpl_fn.into());

            let batch_fn = v8::FunctionTemplate::new(scope, chomp_register_batcher).get_function(scope).unwrap();
            let batcher_key = v8::String::new(scope, "registerBatcher").unwrap();
            chomp_val.set(scope, batcher_key.into(), batch_fn.into());

            let env_key = v8::String::new(scope, "ENV").unwrap();
            let env_val = v8::Object::new(scope);
            global.set(scope, env_key.into(), env_val.into());

            for (key, value) in global_env {
                let env_key = v8::String::new(scope, key).unwrap();
                let env_key_val = v8::String::new(scope, value).unwrap();
                env_val.set(scope, env_key.into(), env_key_val.into());
            }

            v8::Global::new(scope, context)
        };

        let extensions = Extensions::new();
        isolate.set_slot(Rc::new(RefCell::new(extensions)));

        ExtensionEnvironment {
            isolate,
            global_context,
        }
    }

    fn handle_scope(&mut self) -> v8::HandleScope {
        v8::HandleScope::with_context(&mut self.isolate, self.global_context.clone())
    }

    pub fn get_tasks(&self) -> Vec<ChompTaskMaybeTemplatedNoDefault> {
        self.isolate.get_slot::<Rc<RefCell<Extensions>>>().unwrap().borrow().tasks.clone()
    }

    fn get_extensions(&self) -> &Rc<RefCell<Extensions>> {
        self.isolate.get_slot::<Rc<RefCell<Extensions>>>().unwrap()
    }

    pub fn add_extension(&mut self, extension_source: &str, filename: &str) -> Result<()> {
        let mut handle_scope = self.handle_scope();
        let code = v8::String::new(&mut handle_scope, extension_source).unwrap();
        let tc_scope = &mut v8::TryCatch::new(&mut handle_scope);
        let resource_name = v8::String::new(tc_scope, &filename).unwrap().into();
        let source_map = v8::String::new(tc_scope, "").unwrap().into();
        let origin = v8::ScriptOrigin::new(
            tc_scope,
            resource_name,
            0,
            0,
            false,
            123,
            source_map,
            true,
            false,
            false,
        );
        let script = match v8::Script::compile(tc_scope, code, Some(&origin)) {
            Some(script) => script,
            None => return Err(v8_exception(tc_scope)),
        };
        match script.run(tc_scope) {
            Some(_) => {}
            None => return Err(v8_exception(tc_scope)),
        };
        Ok(())
    }

    pub fn run_template(
        &mut self,
        name: &str,
        task: &ChompTaskMaybeTemplatedNoDefault,
    ) -> Result<Vec<ChompTaskMaybeTemplatedNoDefault>> {
        let template = {
            let extensions = self.get_extensions().borrow();
            extensions.templates[name].clone()
        };
        let cb = template.open(&mut self.isolate);

        let mut handle_scope = self.handle_scope();
        let tc_scope = &mut v8::TryCatch::new(&mut handle_scope);

        let this = v8::undefined(tc_scope).into();
        let args: Vec<v8::Local<v8::Value>> = vec![to_v8(tc_scope, task).expect("Unable to serialize template params")];
        let result = match cb.call(tc_scope, this, args.as_slice()) {
            Some(result) => result,
            None => return Err(v8_exception(tc_scope)),
        };
        let task: Vec<ChompTaskMaybeTemplatedNoDefault> = from_v8(tc_scope, result)
            .expect("Unable to deserialize template task list due to invalid structure");
        Ok(task)
    }

    pub fn run_batcher(
        &mut self,
        idx: usize,
        batch: &HashSet<&CmdOp>,
        running: &HashSet<&BatchCmd>,
    ) -> Result<(
        (Vec<usize>, Vec<BatchCmd>, BTreeMap<usize, usize>),
        Option<usize>,
    )> {
        let (_name, batcher, batchers_len) = {
            let extensions = self.get_extensions().borrow();
            let (name, batcher) = extensions.batchers[idx].clone();
            (name, batcher, extensions.batchers.len())
        };
        let cb = batcher.open(&mut self.isolate);

        let mut handle_scope = self.handle_scope();
        let tc_scope = &mut v8::TryCatch::new(&mut handle_scope);

        let this = v8::undefined(tc_scope).into();
        let args: Vec<v8::Local<v8::Value>> = vec![
            to_v8(tc_scope, batch).expect("Unable to serialize batcher call"),
            to_v8(tc_scope, running).expect("Unable to serialize batcher call"),
        ];

        let result = match cb.call(tc_scope, this, args.as_slice()) {
            Some(result) => result,
            None => return Err(v8_exception(tc_scope)),
        };

        let result: (Vec<usize>, Vec<BatchCmd>, BTreeMap<usize, usize>) = from_v8(tc_scope, result)
            .expect("Unable to deserialize batch due to invalid structure");
        let next = if idx < batchers_len - 1 {
            Some(idx + 1)
        } else {
            None
        };
        Ok((result, next))
    }
}

fn v8_exception<'a>(scope: &mut v8::TryCatch<v8::HandleScope>) -> Error {
    let exception = scope.exception().unwrap();
    if is_instance_of_error(scope, exception) {
        let exception: v8::Local<v8::Object> = exception.try_into().unwrap();

        let stack = get_property(scope, exception, "stack");
        let stack: Option<v8::Local<v8::String>> = stack.and_then(|s| s.try_into().ok());
        let stack = stack.map(|s| s.to_rust_string_lossy(scope));
        let err_str = stack.unwrap();
        if err_str.starts_with("Error: ") {
            anyhow!("{}", &err_str[7..])
        } else if err_str.starts_with("TypeError: ") {
            anyhow!("TypeError {}", &err_str[11..])
        } else if err_str.starts_with("SyntaxError: ") {
            anyhow!("SyntaxError {}", &err_str[13..])
        } else if err_str.starts_with("ReferenceError: ") {
            anyhow!("ReferenceError {}", &err_str[16..])
        } else {
            anyhow!("{}", &err_str)
        }
    } else {
        anyhow!("JS error: {}", exception.to_rust_string_lossy(scope))
    }
}

fn get_property<'a>(
    scope: &mut v8::HandleScope<'a>,
    object: v8::Local<v8::Object>,
    key: &str,
) -> Option<v8::Local<'a, v8::Value>> {
    let key = v8::String::new(scope, key).unwrap();
    object.get(scope, key.into())
}

fn is_instance_of_error<'s>(scope: &mut v8::HandleScope<'s>, value: v8::Local<v8::Value>) -> bool {
    if !value.is_object() {
        return false;
    }
    let message = v8::String::empty(scope);
    let error_prototype = v8::Exception::error(scope, message)
        .to_object(scope)
        .unwrap()
        .get_prototype(scope)
        .unwrap();
    let mut maybe_prototype = value.to_object(scope).unwrap().get_prototype(scope);
    while let Some(prototype) = maybe_prototype {
        if prototype.strict_equals(error_prototype) {
            return true;
        }
        maybe_prototype = prototype.to_object(scope).and_then(|o| o.get_prototype(scope));
    }
    false
}