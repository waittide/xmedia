mod ciweimao;
mod common;
mod faloo;
mod fanqie;
mod jjwxc;
mod k17;
mod qidian;
mod qimao;
mod sfacg;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::application::ports::ScraperProviderPort;

pub(super) fn providers() -> Vec<Arc<dyn ScraperProviderPort>> {
    vec![
        Arc::new(qidian::QidianProvider),
        Arc::new(fanqie::FanqieProvider),
        Arc::new(qimao::QimaoProvider),
        Arc::new(k17::K17Provider),
        Arc::new(ciweimao::CiweimaoProvider),
        Arc::new(sfacg::SfacgProvider),
        Arc::new(faloo::FalooProvider),
        Arc::new(jjwxc::JjwxcProvider),
    ]
}
