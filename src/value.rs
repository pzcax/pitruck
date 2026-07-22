use crate::ast::Stmt;
use std::fmt;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::cell::RefCell;

#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    Str(String),
    Bool(bool),
    Null,
    Function { name: String, params: Rc<Vec<(String, u64)>>, body: Rc<Vec<Stmt>>, captured: Rc<RefCell<Vec<(u64, Value)>>>, is_closure: bool },
    List(Rc<RefCell<Vec<Value>>>),
    Dict(Rc<RefCell<HashMap<String, Value>>>),
    Class { name: String, methods: HashMap<String, Value> },
    Instance {
        class_name: String,
        fields: Rc<RefCell<HashMap<String, Value>>>,
        methods: HashMap<String, Value>,
    },
    BoundMethod {
        receiver: Box<Value>,
        method: Box<Value>,
    },
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{n}")
                }
            }
            Value::Str(s)  => write!(f, "{s}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Null    => write!(f, "null"),
            Value::Function { name, .. } => write!(f, "<func {name}>"),
            Value::List(l) => {
                write!(f, "[")?;
                let list = l.borrow();
                for (i, val) in list.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", val)?;
                }
                write!(f, "]")
            }
            Value::Dict(d) => {
                write!(f, "{{")?;
                let dict = d.borrow();
                for (i, (k, v)) in dict.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "\"{}\": {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Class { name, .. }             => write!(f, "<class {name}>"),
            Value::Instance { class_name, .. }    => write!(f, "<instance of {class_name}>"),
            Value::BoundMethod { method, .. }     => write!(f, "<bound method {}>", method),
        }
    }
}

impl Value {
    #[inline(always)]
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Value::Bool(false) | Value::Null)
    }
}