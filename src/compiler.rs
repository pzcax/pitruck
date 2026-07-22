use crate::ast::*;
use ahash::AHashMap as HashMap;

pub struct SlotMap {
    scopes: Vec<HashMap<String, usize>>,
    pub next_slot: usize,
}

impl SlotMap {
    pub fn new() -> Self {
        SlotMap { scopes: vec![HashMap::new()], next_slot: 0 }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            self.next_slot -= scope.len();
        }
    }

    pub fn define(&mut self, name: &str) -> usize {
        let slot = self.next_slot;
        self.next_slot += 1;
        self.scopes.last_mut().unwrap().insert(name.to_string(), slot);
        slot
    }

    pub fn lookup(&self, name: &str) -> Option<usize> {
        for scope in self.scopes.iter().rev() {
            if let Some(&slot) = scope.get(name) {
                return Some(slot);
            }
        }
        None
    }
}

pub fn resolve_program(stmts: &mut Vec<Stmt>) {
    for s in stmts.iter_mut() {
        resolve_stmt_name_hashes(s);
    }
}
fn resolve_stmt_name_hashes(stmt: &mut Stmt) {
    match stmt {
        Stmt::VarDecl { name, hash, value, .. } => {
            *hash = crate::symbol::hash_name(name);
            resolve_expr_name_hashes(value);
        }
        Stmt::Assign { name, hash, value, .. } => {
            *hash = crate::symbol::hash_name(name);
            resolve_expr_name_hashes(value);
        }
        Stmt::Set { object, value, .. } => {
            resolve_expr_name_hashes(object);
            resolve_expr_name_hashes(value);
        }
        Stmt::IndexSet { object, index, value, .. } => {
            resolve_expr_name_hashes(object);
            resolve_expr_name_hashes(index);
            resolve_expr_name_hashes(value);
        }
        Stmt::If { condition, then_branch, elif_branches, else_branch, .. } => {
            resolve_expr_name_hashes(condition);
            for s in then_branch.iter_mut() { resolve_stmt_name_hashes(s); }
            for (c, b) in elif_branches.iter_mut() {
                resolve_expr_name_hashes(c);
                for s in b.iter_mut() { resolve_stmt_name_hashes(s); }
            }
            if let Some(eb) = else_branch {
                for s in eb.iter_mut() { resolve_stmt_name_hashes(s); }
            }
        }
        Stmt::While { condition, body, .. } => {
            resolve_expr_name_hashes(condition);
            for s in body.iter_mut() { resolve_stmt_name_hashes(s); }
        }
        Stmt::For { var, var_hash, iter, body, .. } => {
            *var_hash = crate::symbol::hash_name(var);
            resolve_expr_name_hashes(iter);
            for s in body.iter_mut() { resolve_stmt_name_hashes(s); }
        }
        Stmt::Return { value, .. } => {
            if let Some(e) = value { resolve_expr_name_hashes(e); }
        }
        Stmt::Print { value, .. } => { resolve_expr_name_hashes(value); }
        Stmt::ExprStmt { expr, .. } => { resolve_expr_name_hashes(expr); }
        Stmt::FuncDef { params, body, .. } => {
            for (pname, phash) in params.iter_mut() {
                *phash = crate::symbol::hash_name(pname);
            }
            for s in body.iter_mut() { resolve_stmt_name_hashes(s); }
        }
        Stmt::ClassDef { methods, .. } => {
            for m in methods.iter_mut() { resolve_stmt_name_hashes(m); }
        }
        Stmt::Match { expr, arms, default, .. } => {
            resolve_expr_name_hashes(expr);
            for (ae, ab) in arms.iter_mut() {
                resolve_expr_name_hashes(ae);
                for s in ab.iter_mut() { resolve_stmt_name_hashes(s); }
            }
            if let Some(d) = default {
                for s in d.iter_mut() { resolve_stmt_name_hashes(s); }
            }
        }
        Stmt::Bring { .. } => {}
        Stmt::Break { .. } => {}
        Stmt::Continue { .. } => {}
        Stmt::Try { try_body, catch_var, catch_hash, catch_body, .. } => {
            for s in try_body.iter_mut() { resolve_stmt_name_hashes(s); }
            *catch_hash = crate::symbol::hash_name(catch_var);
            for s in catch_body.iter_mut() { resolve_stmt_name_hashes(s); }
        }
    }
}

fn resolve_expr_name_hashes(expr: &mut Expr) {
    match expr {
        Expr::Ident { name, hash, .. } => { *hash = crate::symbol::hash_name(name); }
        Expr::BinOp { left, right, .. } => {
            resolve_expr_name_hashes(left);
            resolve_expr_name_hashes(right);
        }
        Expr::Unary { expr: inner, .. } => { resolve_expr_name_hashes(inner); }
        Expr::Call { callee, args, .. } => {
            resolve_expr_name_hashes(callee);
            for a in args.iter_mut() { resolve_expr_name_hashes(a); }
        }
        Expr::Get { object, .. } => { resolve_expr_name_hashes(object); }
        Expr::IndexGet { object, index, .. } => {
            resolve_expr_name_hashes(object);
            resolve_expr_name_hashes(index);
        }
        Expr::List { elements, .. } => {
            for e in elements.iter_mut() { resolve_expr_name_hashes(e); }
        }
        Expr::Dict { elements, .. } => {
            for (k, v) in elements.iter_mut() {
                resolve_expr_name_hashes(k);
                resolve_expr_name_hashes(v);
            }
        }
        Expr::Lambda { params, body, .. } => {
            for (pname, phash) in params.iter_mut() {
                *phash = crate::symbol::hash_name(pname);
            }
            for s in body.iter_mut() { resolve_stmt_name_hashes(s); }
        }
        Expr::Self_ { .. } => {}
        Expr::Ternary { cond, then_expr, else_expr, .. } => {
            resolve_expr_name_hashes(cond);
            resolve_expr_name_hashes(then_expr);
            resolve_expr_name_hashes(else_expr);
        }
        Expr::Number(_) | Expr::StringLit(_) | Expr::Bool(_) | Expr::Null => {}
    }
}