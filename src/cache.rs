use crate::ast::Stmt;
use std::sync::Mutex;
use ahash::AHashMap as HashMap;
use std::time::SystemTime;

struct CachedProgram {
    stmts: Vec<Stmt>,
    mtime: SystemTime,
}

pub struct ProgramCache {
    cache: Mutex<HashMap<String, CachedProgram>>,
}

impl ProgramCache {
    pub fn new() -> Self {
        ProgramCache {
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_or_parse(
        &self,
        path: &str,
        source: &str,
    ) -> Result<Vec<Stmt>, String> {
        let current_mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok();

        let mut cache = self.cache.lock().unwrap();

        if let Some(entry) = cache.get(path) {
            if current_mtime.map_or(true, |mt| mt == entry.mtime) {
                return Ok(entry.stmts.clone());
            }
        }

        let mut lex = crate::lexer::Lexer::new(source);
        let tokens = lex.tokenize().map_err(|e| format!("{e}"))?;

        let mut par = crate::parser::Parser::new(tokens);
        let mut program = par.parse_program().map_err(|e| format!("{e}"))?;
        crate::compiler::resolve_program(&mut program);

        cache.insert(path.to_string(), CachedProgram {
            stmts: program.clone(),
            mtime: current_mtime.unwrap_or(SystemTime::UNIX_EPOCH),
        });

        Ok(program)
    }
}
