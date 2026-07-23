use crate::ast::*;
use crate::value::Value;
use crate::error::PitruckError;
use crate::json;
use crate::httpclient;
use crate::store::Store;
use std::collections::HashSet;
use ahash::AHashMap as HashMap;
use std::io::{self, Write, BufRead};
use std::time::{SystemTime, UNIX_EPOCH, Instant};
use std::rc::Rc;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
pub enum Signal {
    None,
    Return(Value),
    Break,
    Continue,
}

pub struct Interpreter {
    vars: Vec<(u64, Value)>,
    scope_tops: Vec<usize>,
    lookup_cache: HashMap<u64, usize>,
    start:          Instant,
    rand_seed:      u64,
    loaded_modules: HashSet<String>,
    allow_read:     bool,
    allow_write:    bool,
    allow_net:      bool,
    script_dir:     Option<PathBuf>,
    exe_dir:        Option<PathBuf>,
    server_store:   Option<Arc<Store>>,
}

impl Interpreter {
    pub fn new() -> Self {
        use crate::symbol::hash_name;
        let mut vars = Vec::with_capacity(1024);
        vars.push((hash_name("PI"), Value::Number(std::f64::consts::PI)));
        vars.push((hash_name("E"),  Value::Number(std::f64::consts::E)));
        Interpreter {
            vars,
            lookup_cache: HashMap::new(),
            scope_tops: { let mut v = Vec::with_capacity(256); v.push(0); v },
            exe_dir: std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())),
            start:  Instant::now(),
            rand_seed: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            loaded_modules: HashSet::new(),
            allow_read: false,
            allow_write: false,
            allow_net: false,
            script_dir: None,
            server_store: None,
        }
    }

    pub fn set_permissions(&mut self, allow_read: bool, allow_write: bool, allow_net: bool) {
        self.allow_read = allow_read;
        self.allow_write = allow_write;
        self.allow_net = allow_net;
    }

    pub fn set_server_store(&mut self, store: Arc<Store>) {
        self.server_store = Some(store);
    }
    
    pub fn inject_request_response(
        &mut self,
        method: &str,
        path: &str,
        query_str: &str,
        query: HashMap<String, Value>,
        form: HashMap<String, Value>,
        body: &str,
        headers: HashMap<String, Value>,
    ) {
        use crate::symbol::hash_name;
        
        let mut request_fields: HashMap<String, Value> = HashMap::new();
        request_fields.insert("method".to_string(), Value::Str(method.to_string()));
        request_fields.insert("path".to_string(), Value::Str(path.to_string()));
        request_fields.insert("query_str".to_string(), Value::Str(query_str.to_string()));
        request_fields.insert("query".to_string(), Value::Dict(Rc::new(RefCell::new(query))));
        request_fields.insert("form".to_string(), Value::Dict(Rc::new(RefCell::new(form))));
        request_fields.insert("body".to_string(), Value::Str(body.to_string()));
        request_fields.insert("headers".to_string(), Value::Dict(Rc::new(RefCell::new(headers))));

        let request = Value::Instance {
            class_name: "__Request".to_string(),
            fields: Rc::new(RefCell::new(request_fields)),
            methods: HashMap::new(),
        };

        let mut response_fields: HashMap<String, Value> = HashMap::new();
        response_fields.insert("status".to_string(), Value::Number(200.0));
        response_fields.insert("body".to_string(), Value::Str(String::new()));
        response_fields.insert("headers".to_string(), Value::Dict(Rc::new(RefCell::new(HashMap::new()))));

        let response = Value::Instance {
            class_name: "__Response".to_string(),
            fields: Rc::new(RefCell::new(response_fields)),
            methods: HashMap::new(),
        };

        self.vars.push((hash_name("request"), request));
        self.vars.push((hash_name("response"), response));
    }

    pub fn set_script_path(&mut self, path: &str) {
        let p = Path::new(path);
        if let Some(dir) = p.parent() {
            self.script_dir = Some(if dir.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                dir.to_path_buf()
            });
        }
    }

    fn resolve_module(&self, module: &str) -> Option<PathBuf> {
        let candidates = self.module_candidates(module);
        candidates.into_iter().find(|p| p.exists())
    }

    fn module_candidates(&self, module: &str) -> Vec<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        let names = if module.ends_with(".pr") {
            vec![module.to_string()]
        } else {
            vec![format!("{}.pr", module), module.to_string()]
        };

        for name in &names {
            let p = Path::new(name);

            if p.is_absolute() {
                candidates.push(p.to_path_buf());
                continue;
            }

            if let Some(ref script_dir) = self.script_dir {
                candidates.push(script_dir.join(name));
                candidates.push(script_dir.join("lib").join(name));
            }

            candidates.push(PathBuf::from(name));
            candidates.push(PathBuf::from("lib").join(name));

            if let Some(ref exe_dir) = self.exe_dir {
                candidates.push(exe_dir.join("lib").join(name));

                let mut global = exe_dir.clone();
                for _ in 0..3 {
                    global = match global.parent() {
                        Some(p) => p.to_path_buf(),
                        None    => break,
                    };
                }
                candidates.push(global.join("lib").join(name));
            }
        }

        candidates
    }

    pub fn read_number(&self, name: &str) -> Option<f64> {
        let hash = crate::symbol::hash_name(name);
        for (k, v) in self.vars.iter().rev() {
            if *k == hash {
                if let Value::Number(n) = v { return Some(*n); }
            }
        }
        None
    }

    pub fn read_string(&self, name: &str) -> Option<String> {
        let hash = crate::symbol::hash_name(name);
        for (k, v) in self.vars.iter().rev() {
            if *k == hash {
                return Some(match v {
                    Value::Str(s) => s.clone(),
                    other         => format!("{}", other),
                });
            }
        }
        None
    }

    pub fn read_response_status(&self) -> Option<f64> {
        let inst = self.get_instance("response")?;
        let fields = inst.borrow();
        match fields.get("status") {
            Some(Value::Number(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn read_response_body(&self) -> Option<String> {
        let inst = self.get_instance("response")?;
        let fields = inst.borrow();
        match fields.get("body") {
            Some(Value::Str(s)) => Some(s.clone()),
            Some(other)         => Some(format!("{}", other)),
            None                => None,
        }
    }

    pub fn read_response_headers(&self) -> Vec<(String, String)> {
        let inst = match self.get_instance("response") {
            Some(i) => i,
            None    => return vec![],
        };
        let fields = inst.borrow();
        match fields.get("headers") {
            Some(Value::Dict(d)) => d.borrow().iter()
                .map(|(k, v)| (k.clone(), format!("{}", v)))
                .collect(),
            _ => vec![],
        }
    }

    fn dict_key_string(&self, v: Value, line: usize) -> Result<String, PitruckError> {
        match v {
            Value::Str(s) => Ok(s),
            Value::Number(n) => Ok(if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{}", n) }),
            Value::Bool(b) => Ok(format!("{}", b)),
            other => Err(PitruckError::RuntimeError { line, message: format!("dict key must be a string, number, or bool, got {}", other) }),
        }
    }

    fn deep_clone(&self, v: &Value) -> Value {
        match v {
            Value::List(l) => {
                let items: Vec<Value> = l.borrow().iter().map(|x| self.deep_clone(x)).collect();
                Value::List(Rc::new(RefCell::new(items)))
            }
            Value::Dict(d) => {
                let mut map = HashMap::new();
                for (k, val) in d.borrow().iter() {
                    map.insert(k.clone(), self.deep_clone(val));
                }
                Value::Dict(Rc::new(RefCell::new(map)))
            }
            other => other.clone(),
        }
    }

    fn error_to_value(&self, e: &PitruckError) -> Value {
        let (etype, msg, err_line) = match e {
            PitruckError::LexError   { line, message, .. } => ("lex", message.clone(), *line),
            PitruckError::ParseError { line, message, .. } => ("parse", message.clone(), *line),
            PitruckError::RuntimeError { line, message }   => ("runtime", message.clone(), *line),
        };
        let mut map = HashMap::new();
        map.insert("type".to_string(), Value::Str(etype.to_string()));
        map.insert("message".to_string(), Value::Str(msg));
        map.insert("line".to_string(), Value::Number(err_line as f64));
        Value::Dict(Rc::new(RefCell::new(map)))
    }

    fn get_instance(&self, name: &str) -> Option<Rc<RefCell<HashMap<String, Value>>>> {
        let hash = crate::symbol::hash_name(name);
        for (k, v) in self.vars.iter().rev() {
            if *k == hash {
                if let Value::Instance { fields, .. } = v {
                    return Some(fields.clone());
                }
            }
        }
        None
    }

    #[inline]
    fn next_rand(&mut self) -> u64 {
        self.rand_seed ^= self.rand_seed << 13;
        self.rand_seed ^= self.rand_seed >> 17;
        self.rand_seed ^= self.rand_seed << 5;
        self.rand_seed
    }

    fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            (Value::Str(x), Value::Str(y)) => x.cmp(y),
            _ => format!("{}", a).cmp(&format!("{}", b)),
        }
    }

    fn call_builtin(&mut self, name: &str, args: &[Value], line: usize) -> Option<Result<Value, PitruckError>> {
        let arity_err = |expected: usize| PitruckError::RuntimeError {
            line,
            message: format!("'{name}' expects {expected} arg(s), got {}", args.len()),
        };

        match name {
            "rand" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                let (a, b) = match (&args[0], &args[1]) {
                    (Value::Number(a), Value::Number(b)) => (*a as i64, *b as i64),
                    _ => return Some(Err(PitruckError::RuntimeError { line, message: "rand requires numbers".to_string() })),
                };
                if a > b { return Some(Err(PitruckError::RuntimeError { line, message: "rand: min must be <= max".to_string() })); }
                let r = (self.next_rand() % (b - a + 1) as u64) as i64 + a;
                Some(Ok(Value::Number(r as f64)))
            }
            "range" => {
                let (start, stop, step) = match args.len() {
                    1 => match &args[0] {
                        Value::Number(n) => (0.0, *n, 1.0),
                        _ => return Some(Err(PitruckError::RuntimeError { line, message: "range requires numbers".to_string() })),
                    },
                    2 => match (&args[0], &args[1]) {
                        (Value::Number(a), Value::Number(b)) => (*a, *b, 1.0),
                        _ => return Some(Err(PitruckError::RuntimeError { line, message: "range requires numbers".to_string() })),
                    },
                    3 => match (&args[0], &args[1], &args[2]) {
                        (Value::Number(a), Value::Number(b), Value::Number(c)) => (*a, *b, *c),
                        _ => return Some(Err(PitruckError::RuntimeError { line, message: "range requires numbers".to_string() })),
                    },
                    _ => return Some(Err(arity_err(1))),
                };
                if step == 0.0 {
                    return Some(Err(PitruckError::RuntimeError { line, message: "range step cannot be zero".to_string() }));
                }
                let mut items = Vec::new();
                let mut cur = start;
                if step > 0.0 {
                    while cur < stop { items.push(Value::Number(cur)); cur += step; }
                } else {
                    while cur > stop { items.push(Value::Number(cur)); cur += step; }
                }
                Some(Ok(Value::List(Rc::new(RefCell::new(items)))))
            }
            "input" => {
                if args.len() > 1 { return Some(Err(arity_err(1))); }
                if let Some(Value::Str(prompt)) = args.first() {
                    print!("{prompt}");
                    io::stdout().flush().ok();
                }
                let mut line_buf = String::new();
                io::stdin().lock().read_line(&mut line_buf).ok();
                let trimmed = line_buf.trim_end_matches('\n').trim_end_matches('\r').to_string();
                Some(Ok(Value::Str(trimmed)))
            }
            "to_number" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Number(n) => Some(Ok(Value::Number(*n))),
                    Value::Str(s)    => match s.trim().parse::<f64>() {
                        Ok(n)  => Some(Ok(Value::Number(n))),
                        Err(_) => Some(Err(PitruckError::RuntimeError { line, message: format!("cannot convert \"{s}\" to number") })),
                    },
                    Value::Bool(b) => Some(Ok(Value::Number(if *b { 1.0 } else { 0.0 }))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "cannot convert null to number".to_string() })),
                }
            }
            "to_string" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                Some(Ok(Value::Str(format!("{}", args[0]))))
            }
            "is_number" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                let ok = match &args[0] {
                    Value::Number(_) => true,
                    Value::Str(s)    => s.trim().parse::<f64>().is_ok(),
                    _                => false,
                };
                Some(Ok(Value::Bool(ok)))
            }
            "html_escape" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                let s = format!("{}", args[0]);
                let escaped = s
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
                    .replace('\'', "&#39;");
                Some(Ok(Value::Str(escaped)))
            }
            "clear" => {
                print!("\x1b[2J\x1b[1;1H");
                io::stdout().flush().ok();
                Some(Ok(Value::Null))
            }
            "len" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(s)    => Some(Ok(Value::Number(s.chars().count() as f64))),
                    Value::List(l)   => Some(Ok(Value::Number(l.borrow().len() as f64))),
                    Value::Dict(d)   => Some(Ok(Value::Number(d.borrow().len() as f64))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "len requires string, list, or dict".to_string() })),
                }
            }
            "push" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match &args[0] {
                    Value::List(l) => { l.borrow_mut().push(args[1].clone()); Some(Ok(Value::Null)) }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "push requires a list".to_string() })),
                }
            }
            "pop" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::List(l) => Some(Ok(l.borrow_mut().pop().unwrap_or(Value::Null))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "pop requires a list".to_string() })),
                }
            }
            "contains" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Str(haystack), Value::Str(needle)) => {
                        Some(Ok(Value::Bool(haystack.contains(needle.as_str()))))
                    }
                    (Value::List(l), needle) => {
                        let found = l.borrow().iter().any(|v| self.values_equal(v, needle));
                        Some(Ok(Value::Bool(found)))
                    }
                    (Value::Dict(d), Value::Str(key)) => {
                        Some(Ok(Value::Bool(d.borrow().contains_key(key.as_str()))))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "contains requires (string, string), (list, value), or (dict, string)".to_string() })),
                }
            }
            "keys" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Dict(d) => {
                        let keys: Vec<Value> = d.borrow().keys().map(|k| Value::Str(k.clone())).collect();
                        Some(Ok(Value::List(Rc::new(RefCell::new(keys)))))
                    }
                    Value::Instance { fields, .. } => {
                        let keys: Vec<Value> = fields.borrow().keys().map(|k| Value::Str(k.clone())).collect();
                        Some(Ok(Value::List(Rc::new(RefCell::new(keys)))))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "keys requires a dict or instance".to_string() })),
                }
            }
            "values" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Dict(d) => {
                        let vals: Vec<Value> = d.borrow().values().cloned().collect();
                        Some(Ok(Value::List(Rc::new(RefCell::new(vals)))))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "values requires a dict".to_string() })),
                }
            }
            "remove" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Dict(d), Value::Str(key)) => {
                        Some(Ok(d.borrow_mut().remove(key.as_str()).unwrap_or(Value::Null)))
                    }
                    (Value::List(l), Value::Number(n)) => {
                        let i = *n as usize;
                        let mut list = l.borrow_mut();
                        if i < list.len() { Some(Ok(list.remove(i))) }
                        else { Some(Err(PitruckError::RuntimeError { line, message: format!("list index {i} out of bounds (len {})", list.len()) })) }
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "remove requires (dict, string) or (list, number)".to_string() })),
                }
            }
            "split" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Str(s), Value::Str(sep)) => {
                        let parts: Vec<Value> = s.split(sep.as_str()).map(|p| Value::Str(p.to_string())).collect();
                        Some(Ok(Value::List(Rc::new(RefCell::new(parts)))))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "split requires (string, string)".to_string() })),
                }
            }
            "join" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::List(l), Value::Str(sep)) => {
                        let parts: Vec<String> = l.borrow().iter().map(|v| format!("{}", v)).collect();
                        Some(Ok(Value::Str(parts.join(sep))))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "join requires (list, string)".to_string() })),
                }
            }
            "trim"    => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(s) => Some(Ok(Value::Str(s.trim().to_string()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "trim requires string".to_string() })),
                }
            }
            "upper"   => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(s) => Some(Ok(Value::Str(s.to_uppercase()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "upper requires string".to_string() })),
                }
            }
            "lower"   => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(s) => Some(Ok(Value::Str(s.to_lowercase()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "lower requires string".to_string() })),
                }
            }
            "replace" => {
                if args.len() != 3 { return Some(Err(arity_err(3))); }
                match (&args[0], &args[1], &args[2]) {
                    (Value::Str(s), Value::Str(from), Value::Str(to)) => {
                        Some(Ok(Value::Str(s.replace(from.as_str(), to.as_str()))))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "replace requires (string, string, string)".to_string() })),
                }
            }
            "starts_with" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Str(s), Value::Str(prefix)) => Some(Ok(Value::Bool(s.starts_with(prefix.as_str())))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "starts_with requires (string, string)".to_string() })),
                }
            }
            "ends_with" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Str(s), Value::Str(suffix)) => Some(Ok(Value::Bool(s.ends_with(suffix.as_str())))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "ends_with requires (string, string)".to_string() })),
                }
            }
            "substr" => {
                if args.len() < 2 || args.len() > 3 { return Some(Err(arity_err(2))); }
                match &args[0] {
                    Value::Str(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        let start = match &args[1] {
                            Value::Number(n) => *n as usize,
                            _ => return Some(Err(PitruckError::RuntimeError { line, message: "substr start must be a number".to_string() })),
                        };
                        let end = if args.len() == 3 {
                            match &args[2] {
                                Value::Number(n) => (start + *n as usize).min(chars.len()),
                                _ => return Some(Err(PitruckError::RuntimeError { line, message: "substr length must be a number".to_string() })),
                            }
                        } else {
                            chars.len()
                        };
                        let result: String = chars.get(start..end).unwrap_or(&[]).iter().collect();
                        Some(Ok(Value::Str(result)))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "substr requires a string".to_string() })),
                }
            }
            "char_at" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Str(s), Value::Number(n)) => {
                        let i = *n as usize;
                        let c = s.chars().nth(i);
                        Some(Ok(match c {
                            Some(ch) => Value::Str(ch.to_string()),
                            None => Value::Null,
                        }))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "char_at requires (string, number)".to_string() })),
                }
            }
            "pad_left" => {
                if args.len() != 3 { return Some(Err(arity_err(3))); }
                match (&args[0], &args[1], &args[2]) {
                    (Value::Str(s), Value::Number(width), Value::Str(fill)) => {
                        let fill_char = fill.chars().next().unwrap_or(' ');
                        let target = *width as usize;
                        let cur = s.chars().count();
                        if cur >= target { Some(Ok(Value::Str(s.clone()))) }
                        else {
                            let pad: String = std::iter::repeat(fill_char).take(target - cur).collect();
                            Some(Ok(Value::Str(pad + s)))
                        }
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "pad_left requires (string, number, string)".to_string() })),
                }
            }
            "pad_right" => {
                if args.len() != 3 { return Some(Err(arity_err(3))); }
                match (&args[0], &args[1], &args[2]) {
                    (Value::Str(s), Value::Number(width), Value::Str(fill)) => {
                        let fill_char = fill.chars().next().unwrap_or(' ');
                        let target = *width as usize;
                        let cur = s.chars().count();
                        if cur >= target { Some(Ok(Value::Str(s.clone()))) }
                        else {
                            let pad: String = std::iter::repeat(fill_char).take(target - cur).collect();
                            Some(Ok(Value::Str(s.clone() + &pad)))
                        }
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "pad_right requires (string, number, string)".to_string() })),
                }
            }
            "repeat_str" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Str(s), Value::Number(n)) => Some(Ok(Value::Str(s.repeat((*n).max(0.0) as usize)))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "repeat_str requires (string, number)".to_string() })),
                }
            }
            "index_of" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match &args[0] {
                    Value::Str(s) => match &args[1] {
                        Value::Str(needle) => {
                            let idx = s.find(needle.as_str()).map(|byte_i| s[..byte_i].chars().count() as f64).unwrap_or(-1.0);
                            Some(Ok(Value::Number(idx)))
                        }
                        _ => Some(Err(PitruckError::RuntimeError { line, message: "index_of on a string requires a string needle".to_string() })),
                    },
                    Value::List(l) => {
                        let pos = l.borrow().iter().position(|v| self.values_equal(v, &args[1]));
                        Some(Ok(Value::Number(pos.map(|p| p as f64).unwrap_or(-1.0))))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "index_of requires a string or list".to_string() })),
                }
            }
            "list_slice" => {
                if args.len() != 3 { return Some(Err(arity_err(3))); }
                match (&args[0], &args[1], &args[2]) {
                    (Value::List(l), Value::Number(start), Value::Number(end)) => {
                        let list = l.borrow();
                        let s = (*start as usize).min(list.len());
                        let e = (*end as usize).min(list.len());
                        if s > e { return Some(Err(PitruckError::RuntimeError { line, message: "list_slice start must be <= end".to_string() })); }
                        Some(Ok(Value::List(Rc::new(RefCell::new(list[s..e].to_vec())))))
                    }
                    (Value::Str(s), Value::Number(start), Value::Number(end)) => {
                        let chars: Vec<char> = s.chars().collect();
                        let st = (*start as usize).min(chars.len());
                        let en = (*end as usize).min(chars.len());
                        if st > en { return Some(Err(PitruckError::RuntimeError { line, message: "list_slice start must be <= end".to_string() })); }
                        Some(Ok(Value::Str(chars[st..en].iter().collect())))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "list_slice requires (list|string, number, number)".to_string() })),
                }
            }
            "list_reverse" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::List(l) => {
                        let mut items = l.borrow().clone();
                        items.reverse();
                        Some(Ok(Value::List(Rc::new(RefCell::new(items)))))
                    }
                    Value::Str(s) => Some(Ok(Value::Str(s.chars().rev().collect()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "list_reverse requires a list or string".to_string() })),
                }
            }
            "list_sort" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::List(l) => {
                        let mut items = l.borrow().clone();
                        items.sort_by(Self::compare_values);
                        Some(Ok(Value::List(Rc::new(RefCell::new(items)))))
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "list_sort requires a list".to_string() })),
                }
            }
            "list_sort_by" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                let list_val = match &args[0] {
                    Value::List(l) => l.clone(),
                    _ => return Some(Err(PitruckError::RuntimeError { line, message: "list_sort_by requires a list as the first argument".to_string() })),
                };
                let cmp_fn = args[1].clone();
                let mut items = list_val.borrow().clone();
                for i in 1..items.len() {
                    let mut j = i;
                    while j > 0 {
                        let result = match self.call_value(cmp_fn.clone(), vec![items[j - 1].clone(), items[j].clone()], line) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        };
                        let should_swap = match result {
                            Value::Number(n) => n > 0.0,
                            _ => return Some(Err(PitruckError::RuntimeError { line, message: "list_sort_by comparator must return a number".to_string() })),
                        };
                        if should_swap { items.swap(j - 1, j); j -= 1; } else { break; }
                    }
                }
                Some(Ok(Value::List(Rc::new(RefCell::new(items)))))
            }
            "list_map" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                let list_val = match &args[0] {
                    Value::List(l) => l.clone(),
                    _ => return Some(Err(PitruckError::RuntimeError { line, message: "list_map requires a list as the first argument".to_string() })),
                };
                let func = args[1].clone();
                let items = list_val.borrow().clone();
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    match self.call_value(func.clone(), vec![item], line) {
                        Ok(v) => out.push(v),
                        Err(e) => return Some(Err(e)),
                    }
                }
                Some(Ok(Value::List(Rc::new(RefCell::new(out)))))
            }
            "list_filter" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                let list_val = match &args[0] {
                    Value::List(l) => l.clone(),
                    _ => return Some(Err(PitruckError::RuntimeError { line, message: "list_filter requires a list as the first argument".to_string() })),
                };
                let func = args[1].clone();
                let items = list_val.borrow().clone();
                let mut out = Vec::new();
                for item in items {
                    match self.call_value(func.clone(), vec![item.clone()], line) {
                        Ok(v) => if v.is_truthy() { out.push(item); },
                        Err(e) => return Some(Err(e)),
                    }
                }
                Some(Ok(Value::List(Rc::new(RefCell::new(out)))))
            }
            "list_reduce" => {
                if args.len() != 3 { return Some(Err(arity_err(3))); }
                let list_val = match &args[0] {
                    Value::List(l) => l.clone(),
                    _ => return Some(Err(PitruckError::RuntimeError { line, message: "list_reduce requires a list as the first argument".to_string() })),
                };
                let func = args[1].clone();
                let mut acc = args[2].clone();
                let items = list_val.borrow().clone();
                for item in items {
                    match self.call_value(func.clone(), vec![acc.clone(), item], line) {
                        Ok(v) => acc = v,
                        Err(e) => return Some(Err(e)),
                    }
                }
                Some(Ok(acc))
            }
            "json_encode" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match json::to_json(&args[0]) {
                    Ok(s)  => Some(Ok(Value::Str(s))),
                    Err(e) => Some(Err(PitruckError::RuntimeError { line, message: e })),
                }
            }
            "json_decode" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(s) => match json::parse_json(s) {
                        Ok(v)  => Some(Ok(v)),
                        Err(e) => Some(Err(PitruckError::RuntimeError { line, message: format!("json_decode: {e}") })),
                    },
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "json_decode requires a string".to_string() })),
                }
            }
            "url_encode" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(s) => Some(Ok(Value::Str(httpclient::url_encode(s)))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "url_encode requires a string".to_string() })),
                }
            }
            "url_decode" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(s) => Some(Ok(Value::Str(httpclient::url_decode(s)))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "url_decode requires a string".to_string() })),
                }
            }
            "http_request" => {
                if !self.allow_net { return Some(Err(PitruckError::RuntimeError { line, message: "outbound network access is not allowed in this context".to_string() })); }
                if args.len() != 4 { return Some(Err(arity_err(4))); }
                let method = match &args[0] { Value::Str(s) => s.clone(), _ => return Some(Err(PitruckError::RuntimeError { line, message: "http_request method must be a string".to_string() })) };
                let url    = match &args[1] { Value::Str(s) => s.clone(), _ => return Some(Err(PitruckError::RuntimeError { line, message: "http_request url must be a string".to_string() })) };
                let body   = match &args[2] { Value::Str(s) => Some(s.clone()), Value::Null => None, _ => return Some(Err(PitruckError::RuntimeError { line, message: "http_request body must be a string".to_string() })) };
                let mut hdrs = Vec::new();
                if let Value::Dict(d) = &args[3] {
                    for (k, v) in d.borrow().iter() {
                        hdrs.push((k.clone(), format!("{}", v)));
                    }
                }
                match httpclient::request(&method, &url, body.as_deref(), &hdrs) {
                    Ok(resp) => {
                        let mut result: HashMap<String, Value> = HashMap::new();
                        result.insert("status".to_string(), Value::Number(resp.status as f64));
                        result.insert("ok".to_string(), Value::Bool(resp.status >= 200 && resp.status < 300));
                        result.insert("body".to_string(), Value::Str(resp.body));
                        let mut headers_map: HashMap<String, Value> = HashMap::new();
                        for (k, v) in resp.headers {
                            headers_map.insert(k, Value::Str(v));
                        }
                        result.insert("headers".to_string(), Value::Dict(Rc::new(RefCell::new(headers_map))));
                        Some(Ok(Value::Dict(Rc::new(RefCell::new(result)))))
                    }
                    Err(e) => Some(Err(PitruckError::RuntimeError { line, message: format!("http_request failed: {e}") })),
                }
            }
            "time"     => {
                let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                Some(Ok(Value::Str(format!("{:02}:{:02}:{:02}", (secs / 3600) % 24, (secs % 3600) / 60, secs % 60))))
            }
            "timestamp" => {
                let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                Some(Ok(Value::Number(secs as f64)))
            }
            "sys_os"   => Some(Ok(Value::Str(std::env::consts::OS.to_string()))),
            "sys_exit" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                let code = match &args[0] { Value::Number(n) => *n as i32, _ => 0 };
                std::process::exit(code);
            }
            "sys_sleep" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                if let Value::Number(n) = args[0] {
                    std::thread::sleep(std::time::Duration::from_millis(n as u64));
                }
                Some(Ok(Value::Null))
            }
            "sys_env" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                let val = match &args[0] {
                    Value::Str(k) => std::env::var(k).unwrap_or_default(),
                    _ => String::new(),
                };
                Some(Ok(Value::Str(val)))
            }
            "sys_writefile" => {
                if !self.allow_write { return Some(Err(PitruckError::RuntimeError { line, message: "file write access is not allowed in this context".to_string() })); }
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Str(path), Value::Str(contents)) => {
                        match fs::write(path.as_str(), contents.as_str()) {
                            Ok(_)  => Some(Ok(Value::Null)),
                            Err(e) => Some(Err(PitruckError::RuntimeError { line, message: format!("write_file '{path}': {e}") })),
                        }
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "sys_writefile(path, contents) requires two strings".to_string() })),
                }
            }
            "sys_readfile" => {
                if !self.allow_read { return Some(Err(PitruckError::RuntimeError { line, message: "file read access is not allowed in this context".to_string() })); }
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(path) => {
                        match fs::read_to_string(path.as_str()) {
                            Ok(s)  => Some(Ok(Value::Str(s))),
                            Err(e) => Some(Err(PitruckError::RuntimeError { line, message: format!("read_file '{path}': {e}") })),
                        }
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "sys_readfile(path) requires a string".to_string() })),
                }
            }
            "sys_fileexists" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(path) => Some(Ok(Value::Bool(Path::new(path.as_str()).exists()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "sys_fileexists(path) requires a string".to_string() })),
                }
            }
            "math_abs"  => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Number(n) => Some(Ok(Value::Number(n.abs()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "abs requires number".to_string() })),
                }
            }
            "math_sqrt" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Number(n) => Some(Ok(Value::Number(n.sqrt()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "sqrt requires number".to_string() })),
                }
            }
            "math_pow"  => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Number(a), Value::Number(b)) => Some(Ok(Value::Number(a.powf(*b)))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "pow requires two numbers".to_string() })),
                }
            }
            "floor" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Number(n) => Some(Ok(Value::Number(n.floor()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "floor requires number".to_string() })),
                }
            }
            "ceil"  => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Number(n) => Some(Ok(Value::Number(n.ceil()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "ceil requires number".to_string() })),
                }
            }
            "round" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Number(n) => Some(Ok(Value::Number(n.round()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "round requires number".to_string() })),
                }
            }
            "clone" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                Some(Ok(self.deep_clone(&args[0])))
            }
            "typeof" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                let t = match &args[0] {
                    Value::Number(_)       => "number",
                    Value::Str(_)          => "string",
                    Value::Bool(_)         => "bool",
                    Value::Null            => "null",
                    Value::List(_)         => "list",
                    Value::Dict(_)         => "dict",
                    Value::Function { .. } => "function",
                    Value::Class { .. }    => "class",
                    Value::Instance { .. } => "instance",
                    Value::BoundMethod { .. } => "function",
                };
                Some(Ok(Value::Str(t.to_string())))
            }
            "server_set" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Str(key), Value::Str(val)) => {
                        if let Some(ref store) = self.server_store {
                            store.set(key, val);
                            Some(Ok(Value::Null))
                        } else {
                            Some(Err(PitruckError::RuntimeError { line, message: "server_set is only available in serve mode".to_string() }))
                        }
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "server_set(key, value) requires two strings".to_string() })),
                }
            }
            "server_get" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(key) => {
                        if let Some(ref store) = self.server_store {
                            match store.get(key) {
                                Some(v) => Some(Ok(Value::Str(v))),
                                None    => Some(Ok(Value::Null)),
                            }
                        } else {
                            Some(Err(PitruckError::RuntimeError { line, message: "server_get is only available in serve mode".to_string() }))
                        }
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "server_get(key) requires a string".to_string() })),
                }
            }
            "server_has" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(key) => {
                        if let Some(ref store) = self.server_store {
                            Some(Ok(Value::Bool(store.has(key))))
                        } else {
                            Some(Err(PitruckError::RuntimeError { line, message: "server_has is only available in serve mode".to_string() }))
                        }
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "server_has(key) requires a string".to_string() })),
                }
            }
            "server_delete" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Str(key) => {
                        if let Some(ref store) = self.server_store {
                            Some(Ok(Value::Bool(store.delete(key))))
                        } else {
                            Some(Err(PitruckError::RuntimeError { line, message: "server_delete is only available in serve mode".to_string() }))
                        }
                    }
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "server_delete(key) requires a string".to_string() })),
                }
            }
            "server_keys" => {
                if args.len() != 0 { return Some(Err(arity_err(0))); }
                if let Some(ref store) = self.server_store {
                    let keys: Vec<Value> = store.keys().iter().map(|k| Value::Str(k.clone())).collect();
                    Some(Ok(Value::List(Rc::new(RefCell::new(keys)))))
                } else {
                    Some(Err(PitruckError::RuntimeError { line, message: "server_keys is only available in serve mode".to_string() }))
                }
            }
            _ => None,
        }
    }

    pub fn run(&mut self, program: &[Stmt]) -> Result<(), PitruckError> {
        for stmt in program {
            match self.exec_stmt(stmt)? {
                Signal::Return(_) => break,
                _ => {}
            }
        }
        Ok(())
    }

    #[inline]
    fn define_hash(&mut self, hash: u64, val: Value) {
        self.vars.push((hash, val));
    }

    #[inline]
    fn define(&mut self, name: &str, val: Value) {
        self.vars.push((crate::symbol::hash_name(name), val));
    }

    fn assign_hash(&mut self, hash: u64, name: &str, val: Value, line: usize) -> Result<(), PitruckError> {
        for (k, v) in self.vars.iter_mut().rev() {
            if *k == hash {
                *v = val;
                return Ok(());
            }
        }
        Err(PitruckError::RuntimeError { line, message: format!("undefined variable '{name}' -- did you mean to use 'var'?") })
    }

    fn assign(&mut self, name: &str, val: Value, line: usize) -> Result<(), PitruckError> {
        self.assign_hash(crate::symbol::hash_name(name), name, val, line)
    }

    fn lookup_hash(&self, hash: u64, name: &str, line: usize) -> Result<Value, PitruckError> {
        for (k, v) in self.vars.iter().rev() {
            if *k == hash { return Ok(v.clone()); }
        }
        Err(PitruckError::RuntimeError { line, message: format!("undefined variable '{name}'") })
    }

    fn lookup(&self, name: &str, line: usize) -> Result<Value, PitruckError> {
        self.lookup_hash(crate::symbol::hash_name(name), name, line)
    }

    #[inline]
    fn push_scope(&mut self) { self.scope_tops.push(self.vars.len()); }

    #[inline]
    fn pop_scope(&mut self) {
        let top = self.scope_tops.pop().unwrap_or(0);
        self.vars.truncate(top);
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Signal, PitruckError> {
        match stmt {
            Stmt::VarDecl { name, hash, value, line } => {
                let v = self.eval_expr(value)?;
                let scope_start = *self.scope_tops.last().unwrap_or(&0);
                for (k, _) in self.vars[scope_start..].iter() {
                    if *k == *hash {
                        return Err(PitruckError::RuntimeError {
                            line: *line,
                            message: format!("'{name}' is already declared in this scope"),
                        });
                    }
                }
                self.vars.push((*hash, v));
                Ok(Signal::None)
            }
            Stmt::Assign { name, hash, value, line } => {
                let v = self.eval_expr(value)?;
                self.assign_hash(*hash, name, v, *line)?;
                Ok(Signal::None)
            }
            Stmt::Set { object, name, value, line } => {
                let obj = self.eval_expr(object)?;
                let val = self.eval_expr(value)?;
                match obj {
                    Value::Instance { fields, .. } => {
                        fields.borrow_mut().insert(name.clone(), val);
                        Ok(Signal::None)
                    }
                    _ => Err(PitruckError::RuntimeError {
                        line: *line,
                        message: format!("cannot set property '{name}' on a non-instance value"),
                    }),
                }
            }
            Stmt::IndexSet { object, index, value, line } => {
                let obj = self.eval_expr(object)?;
                let idx = self.eval_expr(index)?;
                let val = self.eval_expr(value)?;
                match obj {
                    Value::List(list) => {
                        if let Value::Number(n) = idx {
                            let i = n as usize;
                            let mut l = list.borrow_mut();
                            if i < l.len() { l[i] = val; Ok(Signal::None) }
                            else { Err(PitruckError::RuntimeError { line: *line, message: format!("list index {i} out of bounds (len {})", l.len()) }) }
                        } else {
                            Err(PitruckError::RuntimeError { line: *line, message: "list index must be a number".to_string() })
                        }
                    }
                    Value::Dict(dict) => {
                        let k = self.dict_key_string(idx, *line)?;
                        dict.borrow_mut().insert(k, val);
                        Ok(Signal::None)
                    }
                    _ => Err(PitruckError::RuntimeError { line: *line, message: "can only index lists and dicts".to_string() }),
                }
            }
            Stmt::Bring { module, line } => {
                let cache_key = module.clone();
                if self.loaded_modules.contains(&cache_key) { return Ok(Signal::None); }

                let resolved = self.resolve_module(module).ok_or_else(|| {
                    let searched = self.module_candidates(module)
                        .into_iter()
                        .map(|p| format!("  {}", p.display()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    PitruckError::RuntimeError {
                        line: *line,
                        message: format!(
                            "could not bring module '{}' - searched:\n{}",
                            module, searched
                        ),
                    }
                })?;

                let source = fs::read_to_string(&resolved).map_err(|e| PitruckError::RuntimeError {
                    line: *line,
                    message: format!("could not read module '{}': {}", resolved.display(), e),
                })?;

                self.loaded_modules.insert(cache_key);

                let saved_script_dir = self.script_dir.clone();
                if let Some(parent) = resolved.parent() {
                    self.script_dir = Some(if parent.as_os_str().is_empty() {
                        PathBuf::from(".")
                    } else {
                        parent.to_path_buf()
                    });
                }

                let mut lexer  = crate::lexer::Lexer::new(&source);
                let tokens     = lexer.tokenize().map_err(|e| PitruckError::RuntimeError {
                    line: *line,
                    message: format!("while loading module '{}' ({}): {}", module, resolved.display(), e),
                })?;
                let mut parser = crate::parser::Parser::new(tokens);
                let mut program = parser.parse_program().map_err(|e| PitruckError::RuntimeError {
                    line: *line,
                    message: format!("while loading module '{}' ({}): {}", module, resolved.display(), e),
                })?;
                crate::compiler::resolve_program(&mut program);
                self.run(&program).map_err(|e| PitruckError::RuntimeError {
                    line: *line,
                    message: format!("while running module '{}' ({}): {}", module, resolved.display(), e),
                })?;

                self.script_dir = saved_script_dir;

                Ok(Signal::None)
            }
            Stmt::FuncDef { name, params, body, .. } => {
                let func = Value::Function { name: name.clone(), params: Rc::new(params.clone()), body: Rc::new(body.clone()), captured: Rc::new(RefCell::new(Vec::<(u64, Value)>::new())), is_closure: false };
                self.define(name, func);
                Ok(Signal::None)
            }
            Stmt::ClassDef { name, methods, .. } => {
                let mut method_map = HashMap::new();
                for m in methods {
                    if let Stmt::FuncDef { name: mname, params, body, .. } = m {
                        method_map.insert(
                            mname.clone(),
                            Value::Function { name: mname.clone(), params: Rc::new(params.clone()), body: Rc::new(body.clone()), captured: Rc::new(RefCell::new(Vec::<(u64, Value)>::new())), is_closure: false },
                        );
                    }
                }
                self.define(name, Value::Class { name: name.clone(), methods: method_map });
                Ok(Signal::None)
            }
            Stmt::If { condition, then_branch, elif_branches, else_branch, .. } => {
                if self.eval_expr(condition)?.is_truthy() {
                    return self.exec_block(then_branch);
                }
                for (elif_cond, elif_body) in elif_branches {
                    if self.eval_expr(elif_cond)?.is_truthy() {
                        return self.exec_block(elif_body);
                    }
                }
                if let Some(eb) = else_branch { return self.exec_block(eb); }
                Ok(Signal::None)
            }
            Stmt::While { condition, body, .. } => {
                loop {
                    if !self.eval_expr(condition)?.is_truthy() { break; }
                    match self.exec_block(body)? {
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Break => break,
                        Signal::Continue | Signal::None => {}
                    }
                }
                Ok(Signal::None)
            }
            Stmt::For { var, var_hash, iter, body, line } => {
                let iterable = self.eval_expr(iter)?;
                let items = match iterable {
                    Value::List(l) => l.borrow().clone(),
                    Value::Str(s)  => s.chars().map(|c| Value::Str(c.to_string())).collect(),
                    _ => return Err(PitruckError::RuntimeError {
                        line: *line,
                        message: "for-in requires a list or string".to_string(),
                    }),
                };
                for item in items {
                    self.push_scope();
                    self.define_hash(*var_hash, item);
                    let sig = self.exec_block_in_current_scope(body)?;
                    self.pop_scope();
                    match sig {
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Break => break,
                        Signal::Continue | Signal::None => {}
                    }
                }
                Ok(Signal::None)
            }
            Stmt::Match { expr, arms, default, .. } => {
                let val = self.eval_expr(expr)?;
                for (arm_expr, body) in arms {
                    let arm_val = self.eval_expr(arm_expr)?;
                    if self.values_equal(&val, &arm_val) {
                        for s in body {
                            let sig = self.exec_stmt(s)?;
                            if !matches!(sig, Signal::None) { return Ok(sig); }
                        }
                        return Ok(Signal::None);
                    }
                }
                if let Some(def_body) = default {
                    for s in def_body {
                        let sig = self.exec_stmt(s)?;
                        if !matches!(sig, Signal::None) { return Ok(sig); }
                    }
                }
                Ok(Signal::None)
            }
            Stmt::Break { .. } => Ok(Signal::Break),
            Stmt::Continue { .. } => Ok(Signal::Continue),
            Stmt::Try { try_body, catch_hash, catch_body, .. } => {
                match self.exec_block(try_body) {
                    Ok(sig) => Ok(sig),
                    Err(e) => {
                        self.push_scope();
                        let err_val = self.error_to_value(&e);
                        self.define_hash(*catch_hash, err_val);
                        let sig = self.exec_block_in_current_scope(catch_body);
                        self.pop_scope();
                        sig
                    }
                }
            }
            Stmt::Return { value, .. } => {
                let v = if let Some(e) = value { self.eval_expr(e)? } else { Value::Null };
                Ok(Signal::Return(v))
            }
            Stmt::Print { value, .. } => {
                println!("{}", self.eval_expr(value)?);
                Ok(Signal::None)
            }
            Stmt::ExprStmt { expr, .. } => {
                self.eval_expr(expr)?;
                Ok(Signal::None)
            }
        }
    }

    #[inline]
    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Signal, PitruckError> {
        if stmts.is_empty() { return Ok(Signal::None); }
        self.push_scope();
        let result = self.exec_block_in_current_scope(stmts);
        self.pop_scope();
        result
    }

    #[inline(always)]
    fn exec_block_in_current_scope(&mut self, stmts: &[Stmt]) -> Result<Signal, PitruckError> {
        for s in stmts {
            let sig = self.exec_stmt(s)?;
            if !matches!(sig, Signal::None) {
                return Ok(sig);
            }
        }
        Ok(Signal::None)
    }

    fn call_value(&mut self, callee_val: Value, evaluated_args: Vec<Value>, line: usize) -> Result<Value, PitruckError> {
        match callee_val {
            Value::Function { params, body, captured, is_closure, .. } => {
                if evaluated_args.len() != params.len() {
                    return Err(PitruckError::RuntimeError {
                        line,
                        message: format!("expected {} arg(s), got {}", params.len(), evaluated_args.len()),
                    });
                }
                if !is_closure {
                    let scope_base = self.vars.len();
                    self.scope_tops.push(scope_base);
                    self.vars.reserve(params.len());
                    for ((_, ph), arg) in params.iter().zip(evaluated_args) { self.vars.push((*ph, arg)); }
                    let mut ret = Value::Null;
                    let mut call_err = None;
                    'call: for s in body.iter() {
                        match self.exec_stmt(s) {
                            Ok(Signal::Return(v)) => { ret = v; break 'call; }
                            Ok(_) => {}
                            Err(e) => { call_err = Some(e); break 'call; }
                        }
                    }
                    let top = self.scope_tops.pop().unwrap_or(0);
                    self.vars.truncate(top);
                    match call_err {
                        Some(e) => Err(e),
                        None    => Ok(ret),
                    }
                } else {
                    let saved = self.vars.clone();
                    let saved_tops = self.scope_tops.clone();
                    let mut env = captured.borrow().clone();
                    for (k, v) in &self.vars {
                        if !env.iter().any(|(ek, _)| ek == k) {
                            env.push((*k, v.clone()));
                        }
                    }
                    self.vars = env;
                    self.scope_tops = vec![self.vars.len()];
                    self.push_scope();
                    for ((_, ph), arg) in params.iter().zip(evaluated_args) { self.define_hash(*ph, arg); }
                    let mut ret = Value::Null;
                    let mut call_err = None;
                    'call: for s in body.iter() {
                        match self.exec_stmt(s) {
                            Ok(Signal::Return(v)) => { ret = v; break 'call; }
                            Ok(_) => {}
                            Err(e) => { call_err = Some(e); break 'call; }
                        }
                    }
                    let original_len = captured.borrow().len().min(self.vars.len());
                    let updated: Vec<(u64, Value)> = self.vars[..original_len].to_vec();
                    *captured.borrow_mut() = updated;
                    self.pop_scope();
                    self.vars = saved;
                    self.scope_tops = saved_tops;
                    match call_err {
                        Some(e) => Err(e),
                        None    => Ok(ret),
                    }
                }
            }
            Value::Class { name, methods } => {
                let fields = Rc::new(RefCell::new(HashMap::new()));
                let instance = Value::Instance {
                    class_name: name.clone(),
                    fields: fields.clone(),
                    methods: methods.clone(),
                };
                if let Some(Value::Function { params, body, .. }) = methods.get("init") {
                    let body = body.clone();
                    if evaluated_args.len() != params.len() {
                        return Err(PitruckError::RuntimeError {
                            line,
                            message: format!("{name}.init expected {} arg(s), got {}", params.len(), evaluated_args.len()),
                        });
                    }
                    self.push_scope();
                    self.define("self", instance.clone());
                    for ((_, ph), arg) in params.iter().zip(evaluated_args) { self.define_hash(*ph, arg); }
                    let mut init_err = None;
                    for s in body.iter() {
                        match self.exec_stmt(s) {
                            Ok(Signal::Return(_)) => break,
                            Ok(_) => {}
                            Err(e) => { init_err = Some(e); break; }
                        }
                    }
                    self.pop_scope();
                    if let Some(e) = init_err { return Err(e); }
                } else if !evaluated_args.is_empty() {
                    return Err(PitruckError::RuntimeError {
                        line,
                        message: format!("{name} has no init, but {} argument(s) were passed", evaluated_args.len()),
                    });
                }
                Ok(instance)
            }
            Value::BoundMethod { receiver, method } => {
                if let Value::Function { params, body, .. } = *method {
                    if evaluated_args.len() != params.len() {
                        return Err(PitruckError::RuntimeError {
                            line,
                            message: format!("expected {} arg(s), got {}", params.len(), evaluated_args.len()),
                        });
                    }
                    self.push_scope();
                    self.define("self", *receiver);
                    for ((_, ph), arg) in params.iter().zip(evaluated_args) { self.define_hash(*ph, arg); }
                    let mut ret = Value::Null;
                    let mut call_err = None;
                    for s in body.iter() {
                        match self.exec_stmt(s) {
                            Ok(Signal::Return(v)) => { ret = v; break; }
                            Ok(_) => {}
                            Err(e) => { call_err = Some(e); break; }
                        }
                    }
                    self.pop_scope();
                    match call_err {
                        Some(e) => Err(e),
                        None    => Ok(ret),
                    }
                } else {
                    Err(PitruckError::RuntimeError { line, message: "invalid bound method".to_string() })
                }
            }
            _ => Err(PitruckError::RuntimeError { line, message: "value is not callable (expected function or class)".to_string() })
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, PitruckError> {
        match expr {
            Expr::Number(n)    => Ok(Value::Number(*n)),
            Expr::StringLit(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b)      => Ok(Value::Bool(*b)),
            Expr::Null         => Ok(Value::Null),
            Expr::Ident { name, hash, line } => self.lookup_hash(*hash, name, *line),
            Expr::Self_ { line }             => self.lookup_hash(crate::symbol::hash_name("self"), "self", *line),

            Expr::Lambda { params, body, .. } => {
                let mut named_capture: Vec<(u64, Value)> = Vec::new();
                for (k, v) in &self.vars {
                    named_capture.push((*k, v.clone()));
                }
                let captured: Rc<RefCell<Vec<(u64, Value)>>> = Rc::new(RefCell::new(self.vars.clone()));
                Ok(Value::Function { name: "<lambda>".to_string(), params: Rc::new(params.clone()), body: Rc::new(body.clone()), captured, is_closure: true })
            }

            Expr::List { elements, .. } => {
                let mut vec = Vec::with_capacity(elements.len());
                for e in elements { vec.push(self.eval_expr(e)?); }
                Ok(Value::List(Rc::new(RefCell::new(vec))))
            }

            Expr::Dict { elements, line } => {
                let mut map = HashMap::with_capacity(elements.len().max(16));
                for (k, v) in elements {
                    let k_val = self.eval_expr(k)?;
                    let key = self.dict_key_string(k_val, *line)?;
                    map.insert(key, self.eval_expr(v)?);
                }
                Ok(Value::Dict(Rc::new(RefCell::new(map))))
            }

            Expr::Get { object, name, line, optional } => {
                let obj = self.eval_expr(object)?;
                if *optional && matches!(obj, Value::Null) { return Ok(Value::Null); }
                match &obj {
                    Value::Instance { fields, methods, .. } => {
                        if let Some(val) = fields.borrow().get(name) { return Ok(val.clone()); }
                        if let Some(method) = methods.get(name) {
                            return Ok(Value::BoundMethod {
                                receiver: Box::new(obj.clone()),
                                method:   Box::new(method.clone()),
                            });
                        }
                        Err(PitruckError::RuntimeError { line: *line, message: format!("property '{name}' not found on instance") })
                    }
                    Value::Dict(d) => {
                        Ok(d.borrow().get(name).cloned().unwrap_or(Value::Null))
                    }
                    _ => Err(PitruckError::RuntimeError { line: *line, message: format!("cannot access property '{name}' on a non-instance value") }),
                }
            }

            Expr::IndexGet { object, index, line, optional } => {
                let obj = self.eval_expr(object)?;
                if *optional && matches!(obj, Value::Null) { return Ok(Value::Null); }
                let idx = self.eval_expr(index)?;
                match obj {
                    Value::List(list) => {
                        if let Value::Number(n) = idx {
                            let i = n as usize;
                            let l = list.borrow();
                            if i < l.len() { Ok(l[i].clone()) }
                            else { Err(PitruckError::RuntimeError { line: *line, message: format!("list index {i} out of bounds (len {})", l.len()) }) }
                        } else {
                            Err(PitruckError::RuntimeError { line: *line, message: "list index must be a number".to_string() })
                        }
                    }
                    Value::Dict(dict) => {
                        let k = self.dict_key_string(idx, *line)?;
                        Ok(dict.borrow().get(&k).cloned().unwrap_or(Value::Null))
                    }
                    Value::Str(s) => {
                        if let Value::Number(n) = idx {
                            let i = n as usize;
                            match s.chars().nth(i) {
                                Some(c) => Ok(Value::Str(c.to_string())),
                                None    => Err(PitruckError::RuntimeError { line: *line, message: format!("string index {i} out of bounds (len {})", s.chars().count()) }),
                            }
                        } else {
                            Err(PitruckError::RuntimeError { line: *line, message: "string index must be a number".to_string() })
                        }
                    }
                    _ => Err(PitruckError::RuntimeError { line: *line, message: "can only index strings, lists, and dicts".to_string() }),
                }
            }

            Expr::Unary { op, expr, line } => {
                let val = self.eval_expr(expr)?;
                match op {
                    UnaryOpKind::Neg => match val {
                        Value::Number(n) => Ok(Value::Number(-n)),
                        _ => Err(PitruckError::RuntimeError { line: *line, message: "unary '-' requires a number".to_string() }),
                    },
                    UnaryOpKind::Not => Ok(Value::Bool(!val.is_truthy())),
                }
            }

            Expr::BinOp { op, left, right, line } => {
                if matches!(op, BinOpKind::And) {
                    let l = self.eval_expr(left)?;
                    return if !l.is_truthy() { Ok(l) } else { self.eval_expr(right) };
                }
                if matches!(op, BinOpKind::Or) {
                    let l = self.eval_expr(left)?;
                    return if l.is_truthy() { Ok(l) } else { self.eval_expr(right) };
                }
                if matches!(op, BinOpKind::Coalesce) {
                    let l = self.eval_expr(left)?;
                    return if matches!(l, Value::Null) { self.eval_expr(right) } else { Ok(l) };
                }
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                self.apply_binop(op, l, r, *line)
            }

            Expr::Ternary { cond, then_expr, else_expr, .. } => {
                if self.eval_expr(cond)?.is_truthy() {
                    self.eval_expr(then_expr)
                } else {
                    self.eval_expr(else_expr)
                }
            }

            Expr::Call { callee, args, line } => {
                let mut evaluated_args = Vec::with_capacity(args.len());
                for a in args { evaluated_args.push(self.eval_expr(a)?); }

                if let Expr::Ident { name, .. } = &**callee {
                    if name.len() <= 16 {
                        if let Some(result) = self.call_builtin(name, &evaluated_args, *line) {
                            return result;
                        }
                    }
                }

                let callee_val = self.eval_expr(callee)?;
                self.call_value(callee_val, evaluated_args, *line)
            }
        }
    }

    #[inline(always)]
    fn apply_binop(&self, op: &BinOpKind, l: Value, r: Value, line: usize) -> Result<Value, PitruckError> {
        let type_err = |msg: &str| PitruckError::RuntimeError { line, message: msg.to_string() };

        if let (Value::Number(a), Value::Number(b)) = (&l, &r) {
            let a = *a;
            let b = *b;
            return match op {
                BinOpKind::Add  => Ok(Value::Number(a + b)),
                BinOpKind::Sub  => Ok(Value::Number(a - b)),
                BinOpKind::Mul  => Ok(Value::Number(a * b)),
                BinOpKind::Div  => {
                    if b == 0.0 { Err(type_err("division by zero")) }
                    else { Ok(Value::Number(a / b)) }
                }
                BinOpKind::Mod  => Ok(Value::Number(a % b)),
                BinOpKind::Eq   => Ok(Value::Bool(a == b)),
                BinOpKind::NotEq => Ok(Value::Bool(a != b)),
                BinOpKind::Lt   => Ok(Value::Bool(a < b)),
                BinOpKind::Gt   => Ok(Value::Bool(a > b)),
                BinOpKind::LtEq => Ok(Value::Bool(a <= b)),
                BinOpKind::GtEq => Ok(Value::Bool(a >= b)),
                BinOpKind::And | BinOpKind::Or | BinOpKind::Coalesce => unreachable!(),
            };
        }

        match op {
            BinOpKind::Add => match (l, r) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                (Value::Str(a), Value::Str(b)) => {
                    let mut s = String::with_capacity(a.len() + b.len());
                    s.push_str(&a);
                    s.push_str(&b);
                    Ok(Value::Str(s))
                }
                (Value::Str(a), Value::Number(b)) => {
                    let b_str = if b.fract() == 0.0 && b.abs() < 1e15 {
                        format!("{}", b as i64)
                    } else {
                        format!("{}", b)
                    };
                    let mut s = String::with_capacity(a.len() + b_str.len());
                    s.push_str(&a);
                    s.push_str(&b_str);
                    Ok(Value::Str(s))
                }
                (Value::Number(a), Value::Str(b)) => {
                    let a_str = if a.fract() == 0.0 && a.abs() < 1e15 {
                        format!("{}", a as i64)
                    } else {
                        format!("{}", a)
                    };
                    let mut s = String::with_capacity(a_str.len() + b.len());
                    s.push_str(&a_str);
                    s.push_str(&b);
                    Ok(Value::Str(s))
                }
                (Value::List(a), Value::List(b)) => {
                    let mut items = a.borrow().clone();
                    items.extend(b.borrow().iter().cloned());
                    Ok(Value::List(Rc::new(RefCell::new(items))))
                }
                _ => Err(type_err("'+' requires numbers, strings, or lists")),
            },
            BinOpKind::Sub => match (l, r) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a - b)),
                _ => Err(type_err("'-' requires numbers")),
            },
            BinOpKind::Mul => match (l, r) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a * b)),
                _ => Err(type_err("'*' requires numbers")),
            },
            BinOpKind::Div => match (l, r) {
                (Value::Number(_), Value::Number(b)) if b == 0.0 => Err(type_err("division by zero")),
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a / b)),
                _ => Err(type_err("'/' requires numbers")),
            },
            BinOpKind::Mod => match (l, r) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a % b)),
                _ => Err(type_err("'%' requires numbers")),
            },
            BinOpKind::Eq    => Ok(Value::Bool(self.values_equal(&l, &r))),
            BinOpKind::NotEq => Ok(Value::Bool(!self.values_equal(&l, &r))),
            BinOpKind::Lt    => Err(type_err("'<' requires numbers")),
            BinOpKind::Gt    => Err(type_err("'>' requires numbers")),
            BinOpKind::LtEq  => Err(type_err("'<=' requires numbers")),
            BinOpKind::GtEq  => Err(type_err("'>=' requires numbers")),
            BinOpKind::And | BinOpKind::Or | BinOpKind::Coalesce => unreachable!("And/Or/Coalesce short-circuit in eval_expr"),
        }
    }

    #[inline(always)]
    fn values_equal(&self, l: &Value, r: &Value) -> bool {
        match (l, r) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) => true,
            _ => false,
        }
    }
}