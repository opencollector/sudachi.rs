/*
 *  Copyright (c) 2021-2024 Works Applications Co., Ltd.
 *
 *  Licensed under the Apache License, Version 2.0 (the "License");
 *  you may not use this file except in compliance with the License.
 *  You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 *   Unless required by applicable law or agreed to in writing, software
 *  distributed under the License is distributed on an "AS IS" BASIS,
 *  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 *  See the License for the specific language governing permissions and
 *  limitations under the License.
 */

use libloading::{Library, Symbol};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::{SudachiError, SudachiResult};
use crate::plugin::registry::PluginCategory;
use crate::plugin::PluginError;

pub(crate) struct PluginLoader<'a, T: PluginCategory + ?Sized> {
    resolver: Box<dyn Fn(&str) -> Vec<String> + Send + Sync + 'a>,
    libraries: BTreeMap<PathBuf, Arc<Library>>,
    _x: std::marker::PhantomData<T>,
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn make_system_specific_name(s: &str) -> String {
    format!("lib{}.so", s)
}

#[cfg(target_os = "windows")]
fn make_system_specific_name(s: &str) -> String {
    format!("{}.dll", s)
}

#[cfg(target_os = "macos")]
fn make_system_specific_name(s: &str) -> String {
    format!("lib{}.dylib", s)
}

fn system_specific_name(s: &str) -> Option<String> {
    if s.contains('.') {
        None
    } else {
        let p = std::path::Path::new(s);
        let fname = p
            .file_name()
            .and_then(|np| np.to_str())
            .map(make_system_specific_name);
        let parent = p.parent().and_then(|np| np.to_str());
        match (parent, fname) {
            (Some(p), Some(c)) => Some(format!("{}/{}", p, c)),
            _ => None,
        }
    }
}

impl<'a, T: PluginCategory + ?Sized> PluginLoader<'a, T> {
    pub fn new(resolver: impl Fn(&str) -> Vec<String> + 'a + Send + Sync) -> PluginLoader<'a, T> {
        PluginLoader {
            resolver: Box::new(resolver),
            libraries: BTreeMap::new(),
            _x: Default::default(),
        }
    }

    fn resolve_dso_names(&self, name: &str) -> Vec<String> {
        let mut resolved = (self.resolver)(name);

        if let Some(sysname) = system_specific_name(name) {
            resolved.extend((self.resolver)(&sysname));
        }

        resolved
    }

    pub fn load(
        &mut self,
        name: &str,
    ) -> SudachiResult<(<T as PluginCategory>::BoxType, Arc<Library>)> {
        use std::collections::btree_map::Entry::{Occupied, Vacant};

        let candidates = self.resolve_dso_names(name);
        let mut libpath: Option<PathBuf> = None;
        for p in &candidates {
            if fs::metadata(p).is_ok() {
                if let Ok(p) = fs::canonicalize(p) {
                    libpath = Some(p);
                    break;
                }
            }
        }

        if libpath.is_none() {
            return Err(SudachiError::PluginError(PluginError::NotFound {
                name: name.to_owned(),
                candidates: candidates,
            }));
        }

        let libpath = libpath.unwrap();

        let lib = match self.libraries.entry(libpath.clone()) {
            Occupied(entry) => entry.get().clone(),
            Vacant(entry) => {
                let libpath = entry.key();
                let lib = Arc::new(unsafe { Library::new(libpath) }.map_err(|e| {
                    SudachiError::PluginError(PluginError::Libloading {
                        source: e,
                        message: format!("failed to load library from: {:?}", candidates),
                    })
                })?);
                entry.insert(lib).clone()
            }
        };
        let load_fn: Symbol<fn() -> SudachiResult<<T as PluginCategory>::BoxType>> =
            unsafe { lib.get(b"load_plugin") }.map_err(|e| PluginError::Libloading {
                source: e,
                message: format!("no load_plugin symbol in {:?}", libpath),
            })?;
        Ok((load_fn()?, lib))
    }
}
