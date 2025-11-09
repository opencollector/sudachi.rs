/*
 * Copyright (c) 2021 Works Applications Co., Ltd.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use libloading::Error as LLError;
use thiserror::Error;

use crate::config::Config;
use crate::dic::grammar::Grammar;
use crate::error::SudachiResult;
use crate::plugin::connect_cost::EditConnectionCostPlugin;
use crate::plugin::input_text::InputTextPlugin;
use crate::plugin::oov::OovProviderPlugin;
use crate::plugin::path_rewrite::PathRewritePlugin;

pub use self::registry::{FrozenPluginContainer, PluginCategory, PluginContainer, PluginRegistry};

pub mod connect_cost;
pub mod dso;
pub mod input_text;
mod loader;
pub mod oov;
pub mod path_rewrite;
pub mod registry;

#[derive(Error, Debug)]
pub enum PluginError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Plugin {name} not found: {candidates:?}")]
    NotFound {
        name: String,
        candidates: Vec<String>,
    },

    #[error("Libloading Error: {message} ; {source}")]
    Libloading { source: LLError, message: String },

    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error),

    #[error("Invalid data format: {0}")]
    InvalidDataFormat(String),
}

impl From<LLError> for PluginError {
    fn from(e: LLError) -> Self {
        PluginError::Libloading {
            source: e,
            message: String::new(),
        }
    }
}

pub trait Plugins {
    fn connect_cost(&self) -> &dyn PluginContainer<dyn EditConnectionCostPlugin>;
    fn input_text(&self) -> &dyn PluginContainer<dyn InputTextPlugin>;
    fn oov(&self) -> &dyn PluginContainer<dyn OovProviderPlugin>;
    fn path_rewrite(&self) -> &dyn PluginContainer<dyn PathRewritePlugin>;
}

pub struct PluginContainers {
    connect_cost: FrozenPluginContainer<dyn EditConnectionCostPlugin>,
    input_text: FrozenPluginContainer<dyn InputTextPlugin>,
    oov: FrozenPluginContainer<dyn OovProviderPlugin>,
    path_rewrite: FrozenPluginContainer<dyn PathRewritePlugin>,
}

impl PluginContainers {
    /// Helper function to load the plugins of a single category
    /// Should be called with turbofish syntax and trait object type:
    /// `let plugins = load_plugins_of::<dyn InputText>(...)`.
    fn load_plugins_of<'a, 'b, T: PluginCategory + ?Sized>(
        cfg: &'a Config,
        grammar: &'a mut Grammar<'b>,
    ) -> SudachiResult<FrozenPluginContainer<T>> {
        let mut registry = PluginRegistry::new(cfg);
        registry.load_all(grammar)?;
        Ok(registry.into())
    }

    pub fn load<'a, 'b>(cfg: &'a Config, grammar: &'a mut Grammar<'b>) -> SudachiResult<Self>
    where
        'b: 'a,
    {
        Ok(Self {
            connect_cost: Self::load_plugins_of(cfg, grammar)
                .map_err(|e| e.with_context("connect_cost"))?,
            input_text: Self::load_plugins_of(cfg, grammar)
                .map_err(|e| e.with_context("input_text"))?,
            oov: Self::load_plugins_of(cfg, grammar).map_err(|e| e.with_context("oov"))?,
            path_rewrite: Self::load_plugins_of(cfg, grammar)
                .map_err(|e| e.with_context("path_rewrite"))?,
        })
    }
}

impl Plugins for PluginContainers {
    fn connect_cost(&self) -> &dyn PluginContainer<dyn EditConnectionCostPlugin> {
        &self.connect_cost
    }

    fn input_text(&self) -> &dyn PluginContainer<dyn InputTextPlugin> {
        &self.input_text
    }

    fn oov(&self) -> &dyn PluginContainer<dyn OovProviderPlugin> {
        &self.oov
    }

    fn path_rewrite(&self) -> &dyn PluginContainer<dyn PathRewritePlugin> {
        &self.path_rewrite
    }
}

pub struct PluginRegistries<'a> {
    connect_cost: PluginRegistry<'a, dyn EditConnectionCostPlugin>,
    input_text: PluginRegistry<'a, dyn InputTextPlugin>,
    oov: PluginRegistry<'a, dyn OovProviderPlugin>,
    path_rewrite: PluginRegistry<'a, dyn PathRewritePlugin>,
}

impl<'a> Plugins for PluginRegistries<'a> {
    fn connect_cost(&self) -> &dyn PluginContainer<dyn EditConnectionCostPlugin> {
        &self.connect_cost
    }

    fn input_text(&self) -> &dyn PluginContainer<dyn InputTextPlugin> {
        &self.input_text
    }

    fn oov(&self) -> &dyn PluginContainer<dyn OovProviderPlugin> {
        &self.oov
    }

    fn path_rewrite(&self) -> &dyn PluginContainer<dyn PathRewritePlugin> {
        &self.path_rewrite
    }
}
