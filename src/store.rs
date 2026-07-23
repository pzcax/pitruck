use std::sync::Mutex;
use ahash::AHashMap as HashMap;

pub struct Store {
    data: Mutex<HashMap<String, String>>,
}

impl Store {
    pub fn new() -> Self {
        Store {
            data: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.data.lock().unwrap().get(key).cloned()
    }

    pub fn set(&self, key: &str, value: &str) {
        self.data.lock().unwrap().insert(key.to_string(), value.to_string());
    }

    pub fn has(&self, key: &str) -> bool {
        self.data.lock().unwrap().contains_key(key)
    }

    pub fn delete(&self, key: &str) -> bool {
        self.data.lock().unwrap().remove(key).is_some()
    }

    pub fn keys(&self) -> Vec<String> {
        self.data.lock().unwrap().keys().cloned().collect()
    }
}
