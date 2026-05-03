use crate::ast::*;
use crate::value::Value;
use crate::error::PitruckError;
use std::collections::HashMap;
use std::io::{self, Write, Read, BufRead};
use std::time::{SystemTime, UNIX_EPOCH, Instant};
use std::rc::Rc;
use std::cell::RefCell;
use std::fs;

pub enum Signal {
    None,
    Return(Value),
}

pub struct Interpreter {
    scopes: Vec<HashMap<String, Value>>,
    start:  Instant,
    rand_seed: u64,
}

impl Interpreter {
    pub fn new() -> Self {
        let mut globals = HashMap::new();
        globals.insert("PI".to_string(), Value::Number(3.141592653589793));
        globals.insert("E".to_string(), Value::Number(2.718281828459045));
        Interpreter {
            scopes: vec![globals],
            start: Instant::now(),
            rand_seed: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }
    }

    fn next_rand(&mut self) -> u64 {
        self.rand_seed ^= self.rand_seed << 13;
        self.rand_seed ^= self.rand_seed >> 17;
        self.rand_seed ^= self.rand_seed << 5;
        self.rand_seed
    }

    fn call_builtin(&mut self, name: &str, args: &[Value], line: usize) -> Option<Result<Value, PitruckError>> {
        let arity_err = |expected: usize| PitruckError::RuntimeError {
            line,
            message: format!("'{name}' expects {expected} argument(s), got {}", args.len()),
        };

        match name {
            "rand" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                let (a, b) = match (&args[0], &args[1]) {
                    (Value::Number(a), Value::Number(b)) => (*a as i64, *b as i64),
                    _ => return Some(Err(PitruckError::RuntimeError { line, message: "rand requires numbers".to_string() })),
                };
                if a > b { return Some(Err(PitruckError::RuntimeError { line, message: "rand: min <= max".to_string() })); }
                let r = (self.next_rand() % ((b - a + 1) as u64)) as i64 + a;
                Some(Ok(Value::Number(r as f64)))
            }
            "input" => {
                if args.len() > 1 { return Some(Err(arity_err(1))); }
                if let Some(Value::Str(prompt)) = args.first() {
                    print!("{prompt}");
                    io::stdout().flush().ok();
                }
                let mut line_buf = String::new();
                io::stdin().lock().read_line(&mut line_buf).ok();
                Some(Ok(Value::Str(line_buf.trim_end_matches('\n').trim_end_matches('\r').to_string())))
            }
            "to_number" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Number(n) => Some(Ok(Value::Number(*n))),
                    Value::Str(s) => match s.trim().parse::<f64>() {
                        Ok(n)  => Some(Ok(Value::Number(n))),
                        Err(_) => Some(Err(PitruckError::RuntimeError { line, message: "cannot convert to number".to_string() })),
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
            "clear" => {
                print!("\x1b[2J\x1b[1;1H");
                io::stdout().flush().ok();
                Some(Ok(Value::Null))
            }
            "time" => {
                let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                Some(Ok(Value::Str(format!("{:02}:{:02}:{:02}", (secs / 3600) % 24, (secs % 3600) / 60, secs % 60))))
            }
            "sys_os" => {
                Some(Ok(Value::Str(std::env::consts::OS.to_string())))
            }
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
                    Value::Str(k) => std::env::var(k).unwrap_or_else(|_| "".to_string()),
                    _ => "".to_string(),
                };
                Some(Ok(Value::Str(val)))
            }
            "math_abs" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Number(n) => Some(Ok(Value::Number(n.abs()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "requires number".to_string() })),
                }
            }
            "math_sqrt" => {
                if args.len() != 1 { return Some(Err(arity_err(1))); }
                match &args[0] {
                    Value::Number(n) => Some(Ok(Value::Number(n.sqrt()))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "requires number".to_string() })),
                }
            }
            "math_pow" => {
                if args.len() != 2 { return Some(Err(arity_err(2))); }
                match (&args[0], &args[1]) {
                    (Value::Number(a), Value::Number(b)) => Some(Ok(Value::Number(a.powf(*b)))),
                    _ => Some(Err(PitruckError::RuntimeError { line, message: "requires two numbers".to_string() })),
                }
            }
            _ => None,
        }
    }

    pub fn run(&mut self, program: &[Stmt]) -> Result<(), PitruckError> { 
        for stmt in program {
            match self.exec_stmt(stmt)? {
                Signal::Return(_) => break,
                Signal::None      => {}
            }
        }
        Ok(())
    }

    fn define(&mut self, name: &str, val: Value) {
        self.scopes.last_mut().unwrap().insert(name.to_string(), val);
    }

    fn assign(&mut self, name: &str, val: Value, line: usize) -> Result<(), PitruckError> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), val);
                return Ok(());
            }
        }
        Err(PitruckError::RuntimeError { line, message: format!("undefined variable '{name}'") })
    }

    fn lookup(&self, name: &str, line: usize) -> Result<Value, PitruckError> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Ok(v.clone());
            }
        }
        Err(PitruckError::RuntimeError { line, message: format!("undefined variable '{name}'") })
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Signal, PitruckError> {
        match stmt {
            Stmt::VarDecl { name, value, line } => {
                let v = self.eval_expr(value)?;
                if self.scopes.last().unwrap().contains_key(name) {
                    return Err(PitruckError::RuntimeError { line: *line, message: format!("'{name}' is already declared") });
                }
                self.define(name, v);
                Ok(Signal::None)
            }
            Stmt::Assign { name, value, line } => {
                let v = self.eval_expr(value)?;
                self.assign(name, v, *line)?;
                Ok(Signal::None)
            }
            Stmt::Set { object, name, value, line } => {
                let obj = self.eval_expr(object)?;
                let val = self.eval_expr(value)?;
                if let Value::Instance { fields, .. } = obj {
                    fields.borrow_mut().insert(name.clone(), val);
                    Ok(Signal::None)
                } else {
                    Err(PitruckError::RuntimeError { line: *line, message: "only instances have properties".to_string() })
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
                            if i < l.len() {
                                l[i] = val;
                                Ok(Signal::None)
                            } else {
                                Err(PitruckError::RuntimeError { line: *line, message: "index out of bounds".to_string() })
                            }
                        } else {
                            Err(PitruckError::RuntimeError { line: *line, message: "list index must be a number".to_string() })
                        }
                    }
                    Value::Dict(dict) => {
                        if let Value::Str(k) = idx {
                            dict.borrow_mut().insert(k, val);
                            Ok(Signal::None)
                        } else {
                            Err(PitruckError::RuntimeError { line: *line, message: "dict key must be a string".to_string() })
                        }
                    }
                    _ => Err(PitruckError::RuntimeError { line: *line, message: "can only index lists and dicts".to_string() }),
                }
            }
            Stmt::Bring { module, line } => {
                let path = format!("lib/{}.pr", module);
                let source = fs::read_to_string(&path).map_err(|_| PitruckError::RuntimeError {
                    line: *line, message: format!("could not bring module '{module}'")
                })?;
                let mut lexer = crate::lexer::Lexer::new(&source);
                let tokens = lexer.tokenize()?;
                let mut parser = crate::parser::Parser::new(tokens);
                let program = parser.parse_program()?;
                self.run(&program)?;
                Ok(Signal::None)
            }
            Stmt::FuncDef { name, params, body, .. } => {
                let func = Value::Function { name: name.clone(), params: params.clone(), body: body.clone() };
                self.define(name, func);
                Ok(Signal::None)
            }
            Stmt::ClassDef { name, methods, .. } => {
                let mut method_map = HashMap::new();
                for m in methods {
                    if let Stmt::FuncDef { name: mname, params, body, .. } = m {
                        method_map.insert(mname.clone(), Value::Function { name: mname.clone(), params: params.clone(), body: body.clone() });
                    }
                }
                self.define(name, Value::Class { name: name.clone(), methods: method_map });
                Ok(Signal::None)
            }
            Stmt::If { condition, then_branch, elif_branches, else_branch, .. } => {
                let cond_val = self.eval_expr(condition)?;
                if cond_val.is_truthy() {
                    self.push_scope();
                    for s in then_branch {
                        if let Signal::Return(v) = self.exec_stmt(s)? {
                            self.pop_scope();
                            return Ok(Signal::Return(v));
                        }
                    }
                    self.pop_scope();
                    return Ok(Signal::None);
                }
                for (elif_cond, elif_body) in elif_branches {
                    let e_cond = self.eval_expr(elif_cond)?;
                    if e_cond.is_truthy() {
                        self.push_scope();
                        for s in elif_body {
                            if let Signal::Return(v) = self.exec_stmt(s)? {
                                self.pop_scope();
                                return Ok(Signal::Return(v));
                            }
                        }
                        self.pop_scope();
                        return Ok(Signal::None);
                    }
                }
                if let Some(eb) = else_branch {
                    self.push_scope();
                    for s in eb {
                        if let Signal::Return(v) = self.exec_stmt(s)? {
                            self.pop_scope();
                            return Ok(Signal::Return(v));
                        }
                    }
                    self.pop_scope();
                }
                Ok(Signal::None)
            }
            Stmt::While { condition, body, .. } => {
                loop {
                    let c = self.eval_expr(condition)?;
                    if !c.is_truthy() { break; }
                    self.push_scope();
                    for s in body {
                        if let Signal::Return(v) = self.exec_stmt(s)? {
                            self.pop_scope();
                            return Ok(Signal::Return(v));
                        }
                    }
                    self.pop_scope();
                }
                Ok(Signal::None)
            }
            Stmt::Match { expr, arms, default, .. } => {
                let val = self.eval_expr(expr)?;
                let mut matched = false;
                for (arm_expr, body) in arms {
                    let arm_val = self.eval_expr(arm_expr)?;
                    if self.values_equal(&val, &arm_val) {
                        for s in body {
                            match self.exec_stmt(s)? {
                                Signal::Return(v) => return Ok(Signal::Return(v)),
                                Signal::None => {}
                            }
                        }
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    if let Some(def_body) = default {
                        for s in def_body {
                            match self.exec_stmt(s)? {
                                Signal::Return(v) => return Ok(Signal::Return(v)),
                                Signal::None => {}
                            }
                        }
                    }
                }
                Ok(Signal::None)
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

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, PitruckError> {
        match expr {
            Expr::Number(n)    => Ok(Value::Number(*n)),
            Expr::StringLit(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b)      => Ok(Value::Bool(*b)),
            Expr::Null         => Ok(Value::Null),
            Expr::Ident { name, line } => self.lookup(name, *line),
            Expr::Self_ { line } => self.lookup("self", *line),

            Expr::Lambda { params, body, .. } => {
                Ok(Value::Function { name: "<lambda>".to_string(), params: params.clone(), body: body.clone() })
            }

            Expr::List { elements, .. } => {
                let mut vec = Vec::new();
                for e in elements {
                    vec.push(self.eval_expr(e)?);
                }
                Ok(Value::List(Rc::new(RefCell::new(vec))))
            }

            Expr::Dict { elements, line } => {
                let mut map = HashMap::new();
                for (k, v) in elements {
                    let k_val = self.eval_expr(k)?;
                    if let Value::Str(s) = k_val {
                        map.insert(s, self.eval_expr(v)?);
                    } else {
                        return Err(PitruckError::RuntimeError { line: *line, message: "dict key must be string".to_string() });
                    }
                }
                Ok(Value::Dict(Rc::new(RefCell::new(map))))
            }

            Expr::Get { object, name, line } => {
                let obj = self.eval_expr(object)?;
                if let Value::Instance { class_name: _, fields, methods } = obj.clone() {
                    if let Some(val) = fields.borrow().get(name) {
                        return Ok(val.clone());
                    }
                    if let Some(method) = methods.get(name) {
                        return Ok(Value::BoundMethod {
                            receiver: Box::new(obj),
                            method: Box::new(method.clone()),
                        });
                    }
                    Err(PitruckError::RuntimeError { line: *line, message: format!("property '{}' not found", name) })
                } else {
                    Err(PitruckError::RuntimeError { line: *line, message: "only instances have properties".to_string() })
                }
            }

            Expr::IndexGet { object, index, line } => {
                let obj = self.eval_expr(object)?;
                let idx = self.eval_expr(index)?;
                match obj {
                    Value::List(list) => {
                        if let Value::Number(n) = idx {
                            let i = n as usize;
                            let l = list.borrow();
                            if i < l.len() {
                                Ok(l[i].clone())
                            } else {
                                Err(PitruckError::RuntimeError { line: *line, message: "index out of bounds".to_string() })
                            }
                        } else {
                            Err(PitruckError::RuntimeError { line: *line, message: "list index must be a number".to_string() })
                        }
                    }
                    Value::Dict(dict) => {
                        if let Value::Str(k) = idx {
                            if let Some(v) = dict.borrow().get(&k) {
                                Ok(v.clone())
                            } else {
                                Ok(Value::Null)
                            }
                        } else {
                            Err(PitruckError::RuntimeError { line: *line, message: "dict key must be a string".to_string() })
                        }
                    }
                    _ => Err(PitruckError::RuntimeError { line: *line, message: "can only index lists and dicts".to_string() }),
                }
            }

            Expr::Unary { op, expr, line } => {
                let val = self.eval_expr(expr)?;
                match op {
                    UnaryOpKind::Neg => match val {
                        Value::Number(n) => Ok(Value::Number(-n)),
                        _ => Err(PitruckError::RuntimeError { line: *line, message: "'-' requires number".to_string() }),
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
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                self.apply_binop(op, l, r, *line)
            }

            Expr::Call { callee, args, line } => {
                let mut evaluated_args = Vec::new();
                for a in args {
                    evaluated_args.push(self.eval_expr(a)?);
                }

                if let Expr::Ident { name, .. } = &**callee {
                    if let Some(result) = self.call_builtin(name, &evaluated_args, *line) {
                        return result;
                    }
                }

                let callee_val = self.eval_expr(callee)?;

                match callee_val {
                    Value::Function { params, body, .. } => {
                        if evaluated_args.len() != params.len() {
                            return Err(PitruckError::RuntimeError { line: *line, message: format!("expected {} args", params.len()) });
                        }
                        self.push_scope();
                        for (i, p) in params.iter().enumerate() {
                            self.define(p, evaluated_args[i].clone());
                        }
                        let mut ret_val = Value::Null;
                        for s in body {
                            match self.exec_stmt(&s)? {
                                Signal::Return(v) => { ret_val = v; break; },
                                Signal::None => {}
                            }
                        }
                        self.pop_scope();
                        Ok(ret_val)
                    }
                    Value::Class { name, methods } => {
                        let instance = Value::Instance {
                            class_name: name.clone(),
                            fields: Rc::new(RefCell::new(HashMap::new())),
                            methods: methods.clone(),
                        };
                        if let Some(Value::Function { params, body, .. }) = methods.get("init") {
                            if evaluated_args.len() != params.len() {
                                return Err(PitruckError::RuntimeError { line: *line, message: format!("init expected {} args", params.len()) });
                            }
                            self.push_scope();
                            self.define("self", instance.clone());
                            for (i, p) in params.iter().enumerate() {
                                self.define(p, evaluated_args[i].clone());
                            }
                            for s in body {
                                match self.exec_stmt(&s)? {
                                    Signal::Return(_) => break,
                                    Signal::None => {}
                                }
                            }
                            self.pop_scope();
                        } else if !evaluated_args.is_empty() {
                            return Err(PitruckError::RuntimeError { line: *line, message: "class has no init method, but arguments provided".to_string() });
                        }
                        Ok(instance)
                    }
                    Value::BoundMethod { receiver, method } => {
                        if let Value::Function { params, body, .. } = *method {
                            if evaluated_args.len() != params.len() {
                                return Err(PitruckError::RuntimeError { line: *line, message: format!("expected {} args", params.len()) });
                            }
                            self.push_scope();
                            self.define("self", *receiver);
                            for (i, p) in params.iter().enumerate() {
                                self.define(p, evaluated_args[i].clone());
                            }
                            let mut ret_val = Value::Null;
                            for s in body {
                                match self.exec_stmt(&s)? {
                                    Signal::Return(v) => { ret_val = v; break; },
                                    Signal::None => {}
                                }
                            }
                            self.pop_scope();
                            Ok(ret_val)
                        } else {
                            Err(PitruckError::RuntimeError { line: *line, message: "invalid bound method".to_string() })
                        }
                    }
                    _ => Err(PitruckError::RuntimeError { line: *line, message: "callable is not a function or class".to_string() })
                }
            }
        }
    }

    fn apply_binop(&self, op: &BinOpKind, l: Value, r: Value, line: usize) -> Result<Value, PitruckError> {
        let type_err = |msg: &str| PitruckError::RuntimeError { line, message: msg.to_string() };

        match op {
            BinOpKind::Add => match (l, r) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                (Value::Str(a),    Value::Str(b))    => Ok(Value::Str(a + &b)),
                (Value::Str(a),    Value::Number(b)) => Ok(Value::Str(format!("{a}{b}"))),
                (Value::Number(a), Value::Str(b))    => Ok(Value::Str(format!("{a}{b}"))),
                _ => Err(type_err("'+' requires numbers or strings")),
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
            BinOpKind::Lt    => self.compare_nums(l, r, line, |a, b| a < b),
            BinOpKind::Gt    => self.compare_nums(l, r, line, |a, b| a > b),
            BinOpKind::LtEq  => self.compare_nums(l, r, line, |a, b| a <= b),
            BinOpKind::GtEq  => self.compare_nums(l, r, line, |a, b| a >= b),
            _ => Ok(Value::Null),
        }
    }

    fn values_equal(&self, l: &Value, r: &Value) -> bool {
        match (l, r) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Str(a),    Value::Str(b))    => a == b,
            (Value::Bool(a),   Value::Bool(b))   => a == b,
            (Value::Null,      Value::Null)      => true,
            _ => false,
        }
    }

    fn compare_nums<F>(&self, l: Value, r: Value, line: usize, cmp: F) -> Result<Value, PitruckError>
    where
        F: Fn(f64, f64) -> bool,
    {
        match (l, r) {
            (Value::Number(a), Value::Number(b)) => Ok(Value::Bool(cmp(a, b))),
            _ => Err(PitruckError::RuntimeError {
                line,
                message: "comparison requires numbers".to_string(),
            }),
        }
    }
}
