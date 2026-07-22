use crate::value::Value;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::cell::RefCell;

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn format_number(n: f64) -> String {
    if n.is_nan() || n.is_infinite() {
        return "null".to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

pub fn to_json(v: &Value) -> Result<String, String> {
    match v {
        Value::Number(n) => Ok(format_number(*n)),
        Value::Str(s)    => Ok(escape_json_string(s)),
        Value::Bool(b)   => Ok(b.to_string()),
        Value::Null      => Ok("null".to_string()),
        Value::List(l) => {
            let items = l.borrow();
            let mut parts = Vec::with_capacity(items.len());
            for item in items.iter() {
                parts.push(to_json(item)?);
            }
            Ok(format!("[{}]", parts.join(",")))
        }
        Value::Dict(d) => {
            let map = d.borrow();
            let mut parts = Vec::with_capacity(map.len());
            for (k, v) in map.iter() {
                parts.push(format!("{}:{}", escape_json_string(k), to_json(v)?));
            }
            Ok(format!("{{{}}}", parts.join(",")))
        }
        Value::Instance { fields, .. } => {
            let map = fields.borrow();
            let mut parts = Vec::with_capacity(map.len());
            for (k, v) in map.iter() {
                parts.push(format!("{}:{}", escape_json_string(k), to_json(v)?));
            }
            Ok(format!("{{{}}}", parts.join(",")))
        }
        Value::Function { .. } | Value::Class { .. } | Value::BoundMethod { .. } => {
            Err("cannot serialize a function, class, or bound method to JSON".to_string())
        }
    }
}

struct JsonParser {
    chars: Vec<char>,
    pos: usize,
}

impl JsonParser {
    fn new(s: &str) -> Self {
        JsonParser { chars: s.chars().collect(), pos: 0 }
    }

    fn peek(&self) -> Option<char> { self.chars.get(self.pos).copied() }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() { self.pos += 1; }
        c
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\n') | Some('\r')) {
            self.pos += 1;
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<(), String> {
        match self.advance() {
            Some(c) if c == expected => Ok(()),
            Some(c) => Err(format!("expected '{}', found '{}'", expected, c)),
            None    => Err(format!("expected '{}', found end of input", expected)),
        }
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self.peek() {
            Some('"') => self.parse_string_value(),
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('t') => { self.expect_lit("true")?; Ok(Value::Bool(true)) }
            Some('f') => { self.expect_lit("false")?; Ok(Value::Bool(false)) }
            Some('n') => { self.expect_lit("null")?; Ok(Value::Null) }
            Some(c) if c == '-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(format!("unexpected character '{}' in JSON", c)),
            None    => Err("unexpected end of JSON input".to_string()),
        }
    }

    fn expect_lit(&mut self, lit: &str) -> Result<(), String> {
        for expected in lit.chars() {
            match self.advance() {
                Some(c) if c == expected => {}
                _ => return Err(format!("invalid JSON literal, expected '{}'", lit)),
            }
        }
        Ok(())
    }

    fn parse_string_raw(&mut self) -> Result<String, String> {
        self.expect_char('"')?;
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err("unterminated string in JSON".to_string()),
                Some('"') => break,
                Some('\\') => match self.advance() {
                    Some('"')  => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some('/')  => s.push('/'),
                    Some('n')  => s.push('\n'),
                    Some('t')  => s.push('\t'),
                    Some('r')  => s.push('\r'),
                    Some('b')  => s.push('\u{8}'),
                    Some('f')  => s.push('\u{c}'),
                    Some('u')  => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            hex.push(self.advance().ok_or("bad unicode escape in JSON")?);
                        }
                        let code = u32::from_str_radix(&hex, 16).map_err(|_| "bad unicode escape in JSON".to_string())?;
                        if let Some(c) = char::from_u32(code) { s.push(c); }
                    }
                    _ => return Err("bad escape sequence in JSON".to_string()),
                },
                Some(c) => s.push(c),
            }
        }
        Ok(s)
    }

    fn parse_string_value(&mut self) -> Result<Value, String> {
        Ok(Value::Str(self.parse_string_raw()?))
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.peek() == Some('-') { self.pos += 1; }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) { self.pos += 1; }
        if self.peek() == Some('.') {
            self.pos += 1;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) { self.pos += 1; }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+') | Some('-')) { self.pos += 1; }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) { self.pos += 1; }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<f64>().map(Value::Number).map_err(|_| format!("invalid number '{}' in JSON", s))
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        self.expect_char('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') { self.pos += 1; return Ok(Value::List(Rc::new(RefCell::new(items)))); }
        loop {
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.advance() {
                Some(',') => { self.skip_ws(); }
                Some(']') => break,
                Some(c)   => return Err(format!("expected ',' or ']' in JSON array, found '{}'", c)),
                None      => return Err("unterminated array in JSON".to_string()),
            }
        }
        Ok(Value::List(Rc::new(RefCell::new(items))))
    }

    fn parse_object(&mut self) -> Result<Value, String> {
        self.expect_char('{')?;
        let mut map: HashMap<String, Value> = HashMap::new();
        self.skip_ws();
        if self.peek() == Some('}') { self.pos += 1; return Ok(Value::Dict(Rc::new(RefCell::new(map)))); }
        loop {
            self.skip_ws();
            let key = self.parse_string_raw()?;
            self.skip_ws();
            self.expect_char(':')?;
            let val = self.parse_value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.advance() {
                Some(',') => { self.skip_ws(); }
                Some('}') => break,
                Some(c)   => return Err(format!("expected ',' or '}}' in JSON object, found '{}'", c)),
                None      => return Err("unterminated object in JSON".to_string()),
            }
        }
        Ok(Value::Dict(Rc::new(RefCell::new(map))))
    }
}

pub fn parse_json(s: &str) -> Result<Value, String> {
    let mut p = JsonParser::new(s);
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err("trailing characters after JSON value".to_string());
    }
    Ok(v)
}