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

use libloading::Library;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::config::{Config, ConfigError};
use crate::dic::grammar::Grammar;
use crate::error::{SudachiError, SudachiResult};
use crate::plugin::loader::PluginLoader;

/// A category of Plugins
pub trait PluginCategory {
    /// Boxed type of the plugin. Should be Box<dyn XXXX>.
    type BoxType;

    /// Type of the initialization function.
    /// It must take 0 arguments and return `SudachiResult<Self::BoxType>`.
    type InitFnType;

    /// Extract plugin configurations from the config
    fn configurations(cfg: &Config) -> &[Value];

    /// Create bundled plugin for plugin name
    /// Instead of full name like com.worksap.nlp.sudachi.ProlongedSoundMarkPlugin
    /// should handle only the short one: ProlongedSoundMarkPlugin
    ///
    /// com.worksap.nlp.sudachi. (last dot included) will be stripped automatically
    /// by the loader code
    fn bundled_impl(name: &str) -> Option<Self::BoxType>;

    /// Perform initial setup.
    /// We can't call set_up of the plugin directly in the default implementation
    /// of this method because we do not know the specific type yet
    fn do_setup(
        ptr: &mut Self::BoxType,
        settings: &Value,
        config: &Config,
        grammar: &mut Grammar,
    ) -> SudachiResult<()>;
}

pub trait PluginContainer<T: PluginCategory + ?Sized> {
    fn is_empty(&self) -> bool;

    fn iter<'b>(&'b self) -> Box<dyn Iterator<Item = &<T as PluginCategory>::BoxType> + 'b>;
}

impl<'a, T: PluginCategory + ?Sized + 'static> IntoIterator for &'a dyn PluginContainer<T> {
    type Item = &'a <T as PluginCategory>::BoxType;
    type IntoIter = Box<dyn Iterator<Item = &'a <T as PluginCategory>::BoxType> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

fn extract_plugin_class(val: &Value) -> SudachiResult<&str> {
    let obj = match val {
        Value::Object(v) => v,
        o => {
            return Err(SudachiError::ConfigError(ConfigError::InvalidFormat(
                format!("plugin config must be an object, was {}", o),
            )));
        }
    };
    match obj.get("class") {
        Some(Value::String(v)) => Ok(v),
        _ => Err(SudachiError::ConfigError(ConfigError::InvalidFormat(
            "plugin config must have 'class' key to indicate plugin SO file".to_owned(),
        ))),
    }
}

pub struct PluginRegistry<'a, T: PluginCategory + ?Sized> {
    cfg: &'a Config,
    loader: PluginLoader<'a, T>,
    factories:
        BTreeMap<String, Box<dyn Fn(&Value) -> SudachiResult<<T as PluginCategory>::BoxType>>>,
    plugins: BTreeMap<String, (<T as PluginCategory>::BoxType, Option<Arc<Library>>)>,
}

impl<'a, T: PluginCategory + ?Sized> PluginRegistry<'a, T> {
    pub fn new(cfg: &'a Config) -> PluginRegistry<'a, T> {
        PluginRegistry {
            cfg,
            loader: PluginLoader::new(|path| cfg.resolve_paths(path)),
            factories: BTreeMap::new(),
            plugins: BTreeMap::new(),
        }
    }

    pub fn register_plugin(
        &mut self,
        name: impl AsRef<str>,
        p: impl Fn(&Value) -> SudachiResult<<T as PluginCategory>::BoxType> + 'static,
    ) -> () {
        self.factories.insert(name.as_ref().to_owned(), Box::new(p));
    }

    fn drain(self) -> Vec<(<T as PluginCategory>::BoxType, Option<Arc<Library>>)> {
        self.plugins.into_iter().map(|(_, v)| v).collect()
    }

    fn load_plugin_inner(
        &mut self,
        plugin_cfg: &Value,
    ) -> SudachiResult<(
        String,
        (<T as PluginCategory>::BoxType, Option<Arc<Library>>),
    )> {
        let name = extract_plugin_class(plugin_cfg)?;

        if let Some(f) = self.factories.get(name) {
            let p = (f)(plugin_cfg)?;
            return Ok((name.to_owned(), (p, None)));
        }

        // Try to load bundled plugin first, if its name looks like it
        if let Some(stripped_name) = name.strip_prefix("com.worksap.nlp.sudachi.") {
            if let Some(p) = <T as PluginCategory>::bundled_impl(stripped_name) {
                return Ok((name.to_owned(), (p, None)));
            }
        }

        // Otherwise treat name as DSO
        Ok((
            name.to_owned(),
            self.loader.load(name).map(|(p, lib)| (p, Some(lib)))?,
        ))
    }

    fn load_plugin<'c>(
        &mut self,
        plugin_cfg: &Value,
        grammar: &mut Grammar<'c>,
    ) -> SudachiResult<(
        String,
        (<T as PluginCategory>::BoxType, Option<Arc<Library>>),
    )> {
        let (name, mut p) = self.load_plugin_inner(plugin_cfg)?;
        <T as PluginCategory>::do_setup(&mut p.0, plugin_cfg, self.cfg, grammar)
            .map_err(|e| e.with_context(format!("plugin {} setup", name)))?;
        Ok((name, p))
    }

    pub fn load_all<'c>(&mut self, grammar: &mut Grammar<'c>) -> SudachiResult<&mut Self>
    where
        'c: 'a,
    {
        let configs = <T as PluginCategory>::configurations(self.cfg);
        for cfg in configs {
            let (name, p) = self.load_plugin(cfg, grammar)?;
            self.plugins.insert(name, p);
        }
        Ok(self)
    }
}

impl<'a, T: PluginCategory + ?Sized> PluginContainer<T> for PluginRegistry<'a, T> {
    fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    fn iter<'b>(&'b self) -> Box<dyn Iterator<Item = &'b <T as PluginCategory>::BoxType> + 'b> {
        Box::new(self.into_iter())
    }
}

impl<'a, 'b, T: PluginCategory + ?Sized> IntoIterator for &'b PluginRegistry<'a, T>
where
    'a: 'b,
{
    type Item = &'b <T as PluginCategory>::BoxType;
    type IntoIter = std::iter::Map<
        std::collections::btree_map::Iter<
            'b,
            String,
            (<T as PluginCategory>::BoxType, Option<Arc<Library>>),
        >,
        fn(
            v: (
                &'b String,
                &'b (<T as PluginCategory>::BoxType, Option<Arc<Library>>),
            ),
        ) -> Self::Item,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.plugins.iter().map(|(_, v)| &v.0)
    }
}

/// Holds loaded plugins, whether they are bundled
/// or loaded from DSOs
pub struct FrozenPluginContainer<T: PluginCategory + ?Sized> {
    plugins: Vec<(<T as PluginCategory>::BoxType, Option<Arc<Library>>)>,
}

impl<T: PluginCategory + ?Sized> FrozenPluginContainer<T> {
    fn new(plugins: Vec<(<T as PluginCategory>::BoxType, Option<Arc<Library>>)>) -> Self {
        Self { plugins }
    }
}

impl<'a, T: PluginCategory + ?Sized> From<PluginRegistry<'a, T>> for FrozenPluginContainer<T> {
    fn from(registry: PluginRegistry<'a, T>) -> FrozenPluginContainer<T> {
        Self::new(registry.drain())
    }
}

impl<T: PluginCategory + ?Sized> PluginContainer<T> for FrozenPluginContainer<T> {
    fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    fn iter<'b>(&'b self) -> Box<dyn Iterator<Item = &<T as PluginCategory>::BoxType> + 'b> {
        Box::new(self.into_iter())
    }
}

impl<'a, T: PluginCategory + ?Sized> IntoIterator for &'a FrozenPluginContainer<T> {
    type Item = &'a <T as PluginCategory>::BoxType;
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (<T as PluginCategory>::BoxType, Option<Arc<Library>>)>,
        fn(v: &'a (<T as PluginCategory>::BoxType, Option<Arc<Library>>)) -> Self::Item,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.plugins.iter().map(|v| &v.0)
    }
}
