use crate::ast::*;
use std::collections::HashMap;

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
        resolve_top(s);
    }
}

fn resolve_top(stmt: &mut Stmt) {
    match stmt {
        Stmt::FuncDef { params, body, .. } => {
            let mut func_slots = SlotMap::new();
            for (pname, phash) in params.iter_mut() {
                let s = func_slots.define(pname);
                *phash = s as u64;
            }
            for s in body.iter_mut() {
                resolve_stmt(s, &mut func_slots);
            }
        }
        Stmt::ClassDef { methods, .. } => {
            for m in methods.iter_mut() {
                resolve_top(m);
            }
        }
        Stmt::If { condition, then_branch, elif_branches, else_branch, .. } => {
            resolve_expr(condition, &mut SlotMap::new());
            for s in then_branch.iter_mut() { resolve_top(s); }
            for (cond, body) in elif_branches.iter_mut() {
                resolve_expr(cond, &mut SlotMap::new());
                for s in body.iter_mut() { resolve_top(s); }
            }
            if let Some(eb) = else_branch {
                for s in eb.iter_mut() { resolve_top(s); }
            }
        }
        _ => {}
    }
}

fn resolve_stmt(stmt: &mut Stmt, slots: &mut SlotMap) {
    match stmt {
        Stmt::VarDecl { name, hash, value, .. } => {
            resolve_expr(value, slots);
            let slot = slots.define(name);
            *hash = slot as u64;
        }
        Stmt::Assign { name, hash, .. } => {
            if let Some(slot) = slots.lookup(name) {
                *hash = slot as u64;
            }
            if let Stmt::Assign { value, .. } = stmt {
                resolve_expr(value, slots);
            }
        }
        Stmt::Set { object, value, .. } => {
            resolve_expr(object, slots);
            resolve_expr(value, slots);
        }
        Stmt::IndexSet { object, index, value, .. } => {
            resolve_expr(object, slots);
            resolve_expr(index, slots);
            resolve_expr(value, slots);
        }
        Stmt::FuncDef { params, body, .. } => {
            let mut func_slots = SlotMap::new();
            for (pname, phash) in params.iter_mut() {
                let s = func_slots.define(pname);
                *phash = s as u64;
            }
            for s in body.iter_mut() {
                resolve_stmt(s, &mut func_slots);
            }
        }
        Stmt::ClassDef { name, methods, .. } => {
            let _ = slots.define(name);
            for m in methods.iter_mut() {
                resolve_stmt(m, slots);
            }
        }
        Stmt::If { condition, then_branch, elif_branches, else_branch, .. } => {
            resolve_expr(condition, slots);
            slots.push_scope();
            for s in then_branch.iter_mut() { resolve_stmt(s, slots); }
            slots.pop_scope();
            for (cond, body) in elif_branches.iter_mut() {
                resolve_expr(cond, slots);
                slots.push_scope();
                for s in body.iter_mut() { resolve_stmt(s, slots); }
                slots.pop_scope();
            }
            if let Some(eb) = else_branch {
                slots.push_scope();
                for s in eb.iter_mut() { resolve_stmt(s, slots); }
                slots.pop_scope();
            }
        }
        Stmt::While { condition, body, .. } => {
            resolve_expr(condition, slots);
            slots.push_scope();
            for s in body.iter_mut() { resolve_stmt(s, slots); }
            slots.pop_scope();
        }
        Stmt::For { var, var_hash, iter, body, .. } => {
            resolve_expr(iter, slots);
            slots.push_scope();
            let s = slots.define(var);
            *var_hash = s as u64;
            for st in body.iter_mut() { resolve_stmt(st, slots); }
            slots.pop_scope();
        }
        Stmt::Match { expr, arms, default, .. } => {
            resolve_expr(expr, slots);
            for (arm_expr, body) in arms.iter_mut() {
                resolve_expr(arm_expr, slots);
                slots.push_scope();
                for s in body.iter_mut() { resolve_stmt(s, slots); }
                slots.pop_scope();
            }
            if let Some(def) = default {
                slots.push_scope();
                for s in def.iter_mut() { resolve_stmt(s, slots); }
                slots.pop_scope();
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(e) = value { resolve_expr(e, slots); }
        }
        Stmt::Print { value, .. } => {
            resolve_expr(value, slots);
        }
        Stmt::ExprStmt { expr, .. } => {
            resolve_expr(expr, slots);
        }
        Stmt::Bring { .. } => {}
    }
}

fn resolve_expr(expr: &mut Expr, slots: &mut SlotMap) {
    match expr {
        Expr::Ident { name, hash, .. } => {
            if let Some(slot) = slots.lookup(name) {
                *hash = slot as u64;
            }
        }
        Expr::BinOp { left, right, .. } => {
            resolve_expr(left, slots);
            resolve_expr(right, slots);
        }
        Expr::Unary { expr: inner, .. } => {
            resolve_expr(inner, slots);
        }
        Expr::Call { callee, args, .. } => {
            resolve_expr(callee, slots);
            for a in args.iter_mut() { resolve_expr(a, slots); }
        }
        Expr::Get { object, .. } => {
            resolve_expr(object, slots);
        }
        Expr::IndexGet { object, index, .. } => {
            resolve_expr(object, slots);
            resolve_expr(index, slots);
        }
        Expr::List { elements, .. } => {
            for e in elements.iter_mut() { resolve_expr(e, slots); }
        }
        Expr::Dict { elements, .. } => {
            for (k, v) in elements.iter_mut() {
                resolve_expr(k, slots);
                resolve_expr(v, slots);
            }
        }
        Expr::Lambda { params, body, .. } => {
            let mut func_slots = SlotMap::new();
            for (pname, phash) in params.iter_mut() {
                let s = func_slots.define(pname);
                *phash = s as u64;
            }
            for s in body.iter_mut() { resolve_stmt(s, &mut func_slots); }
        }
        Expr::Self_ { .. } => {}
        Expr::Number(_) | Expr::StringLit(_) | Expr::Bool(_) | Expr::Null => {}
    }
}