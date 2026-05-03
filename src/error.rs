use std::fmt;

#[derive(Debug)]
pub enum PitruckError {
    LexError { line: usize, col: usize, message: String },
    ParseError { line: usize, col: usize, message: String },
    RuntimeError { line: usize, message: String },
}

impl fmt::Display for PitruckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PitruckError::LexError { line, col, message } =>
                write!(f, "[Pitruck Lex Error] Line {line}, Col {col}: {message}"),
            PitruckError::ParseError { line, col, message } =>
                write!(f, "[Pitruck Parse Error] Line {line}, Col {col}: {message}"),
            PitruckError::RuntimeError { line, message } =>
                write!(f, "[Pitruck Runtime Error] Line {line}: {message}"),
        }
    }
}
