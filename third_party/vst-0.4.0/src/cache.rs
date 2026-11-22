use std::sync::Arc;

use crate::{editor::Editor, prelude::*};

pub(crate) struct PluginCache {
    pub info: Info,
    pub params: Arc<dyn PluginParameters>,
    pub editor: Option<Box<dyn Editor>>,
}

impl PluginCache {
    pub fn new(info: &Info, params: Arc<dyn PluginParameters>, editor: Option<Box<dyn Editor>>) -> Self {
        Self {
            info: info.clone(),
            params,
            editor,
        }
    }
}

/// Łatka Szkło: wtyczka i jej pamięć podręczna w jednym obiekcie wskazywanym przez `AEffect.object`.
/// Pole `AEffect.user` należy według VST 2.4 do hosta – Equalizer APO w audiodg.exe je nadpisuje,
/// przez co oryginalny crate czytał liczbę kanałów (i wskaźnik parametrów!) ze śmieci hosta.
pub(crate) struct PluginHolder {
    pub plugin: Box<dyn crate::plugin::Plugin>,
    pub cache: PluginCache,
}
