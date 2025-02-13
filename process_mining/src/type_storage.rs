use std::collections::HashMap;
use std::sync::{RwLock};
use lazy_static::lazy_static;

pub struct TypeStorage {
    types: HashMap<String, usize>,
    ids: Vec<Option<String>>,
}

impl TypeStorage {
    pub fn new() -> Self {
        TypeStorage {
            types: HashMap::new(),
            ids: Vec::new(),
        }
    }

    pub fn get_type_id(&mut self, type_name: &str) -> usize {
        if let Some(&type_id) = self.types.get(type_name) {
            type_id
        } else {
            let type_id = self.ids.len();
            self.types.insert(type_name.to_string(), type_id);
            self.ids.push(Some(type_name.to_string()));
            type_id
        }
    }

    pub fn get_type_name(&self, type_id: usize) -> Option<&str> {
        self.ids.get(type_id).and_then(|opt| opt.as_deref())
    }
}

lazy_static! {
    pub static ref TYPE_STORAGE: RwLock<TypeStorage> = RwLock::new(TypeStorage::new());
}