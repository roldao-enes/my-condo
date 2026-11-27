//! The Fornecedores screen: providers of services, their contacts and the names they appear
//! under in the bank statement.

use std::cell::RefCell;
use std::rc::Rc;

use condo_app::audit::quote;
use condo_core::providers::ProviderKind;
use condo_core::tags::{MatchType, TagCategory};
use condo_db::providers::{self, ProviderInput, ProviderTagInput};
use condo_db::ServiceError;
use slint::ComponentHandle;

use super::{model, strings, Controller, UNEXPECTED};
use crate::viewmodel::providers as vm;
use crate::{AppWindow, Backend, ProviderDraft, ProviderRow, ProviderTagDraft, ProviderTagRow};

const MATCH_TYPES: [MatchType; 2] = [MatchType::Exact, MatchType::Contains];
const CATEGORIES: [TagCategory; 2] = [TagCategory::Recurring, TagCategory::Extraordinary];

impl Controller {
    pub(super) fn refresh_providers(&mut self, ui: &AppWindow) {
        let backend = ui.global::<Backend>();
        let Ok(list) = providers::list(&self.conn) else {
            self.notice(ui, "error", UNEXPECTED);
            return;
        };
        backend.set_provider_kind_options(strings(
            ProviderKind::ALL.iter().map(|k| k.label().to_owned()),
        ));
        backend.set_providers_subtitle(vm::subtitle(list.len()).into());
        backend.set_providers(model(
            vm::rows(&list)
                .iter()
                .map(|p| ProviderRow {
                    id: p.id as i32,
                    name: p.name.clone().into(),
                    kind: p.kind.into(),
                    contact: p.contact.clone().into(),
                    email: p.email.clone().into(),
                    phone: p.phone.clone().into(),
                    iban: p.iban.clone().into(),
                    note: p.note.clone().into(),
                    tags: model(
                        p.names
                            .iter()
                            .map(|t| ProviderTagRow {
                                id: t.id as i32,
                                pattern: t.pattern.clone().into(),
                                matching: t.matching.into(),
                                category: t.category.into(),
                            })
                            .collect(),
                    ),
                })
                .collect(),
        ));
    }

    /// `id` of -1 starts a new provider.
    pub fn open_provider(&mut self, ui: &AppWindow, id: i32) {
        let backend = ui.global::<Backend>();
        backend.set_provider_kind_options(strings(
            ProviderKind::ALL.iter().map(|k| k.label().to_owned()),
        ));
        let draft = if id < 0 {
            // Offer the first kind that has no provider yet.
            let taken: Vec<ProviderKind> = providers::list(&self.conn)
                .unwrap_or_default()
                .iter()
                .map(|p| p.kind)
                .collect();
            let free = ProviderKind::ALL
                .iter()
                .position(|k| !k.is_unique() || !taken.contains(k))
                .unwrap_or(0);
            ProviderDraft {
                id: -1,
                kind_index: free as i32,
                name: "".into(),
                contact: "".into(),
                email: "".into(),
                phone: "".into(),
                iban: "".into(),
                note: "".into(),
            }
        } else {
            let Some(p) = providers::list(&self.conn)
                .unwrap_or_default()
                .into_iter()
                .find(|p| p.id == i64::from(id))
            else {
                return;
            };
            ProviderDraft {
                id,
                kind_index: ProviderKind::ALL
                    .iter()
                    .position(|k| *k == p.kind)
                    .unwrap_or(0) as i32,
                name: p.name.clone().into(),
                contact: p.contact_name.clone().unwrap_or_default().into(),
                email: p.email.clone().unwrap_or_default().into(),
                phone: p.phone.clone().unwrap_or_default().into(),
                iban: p.iban_display().unwrap_or_default().into(),
                note: p.note.clone().unwrap_or_default().into(),
            }
        };
        backend.set_provider_draft(draft);
        self.open_dialog(ui, "provider");
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_provider(
        &mut self,
        ui: &AppWindow,
        id: i32,
        kind: i32,
        name: &str,
        contact: &str,
        email: &str,
        phone: &str,
        iban: &str,
        note: &str,
    ) {
        let existing = (id >= 0).then_some(i64::from(id));
        let input = ProviderInput {
            kind: ProviderKind::ALL
                .get(kind.max(0) as usize)
                .copied()
                .unwrap_or(ProviderKind::Other),
            name: name.to_owned(),
            contact_name: contact.to_owned(),
            email: email.to_owned(),
            phone: phone.to_owned(),
            iban: iban.to_owned(),
            note: note.to_owned(),
        };
        let event = if existing.is_some() {
            "provider.update"
        } else {
            "provider.create"
        };
        // The IBAN and contact details stay out of the audit log.
        let summary = format!("{} {}", input.kind.as_str(), quote(name.trim(), 160));
        self.mutate(
            ui,
            event,
            &format!("save the provider {summary}"),
            "Fornecedor guardado.",
            move |conn, actor, _| match existing {
                Some(id) => providers::update(conn, actor, id, &input).map(|()| id),
                None => providers::create(conn, actor, &input),
            },
            move |id| format!("saved provider #{id} ({summary})"),
        );
    }

    pub fn open_provider_remove(&mut self, ui: &AppWindow, id: i32) {
        let Some(p) = providers::list(&self.conn)
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.id == i64::from(id))
        else {
            return;
        };
        let backend = ui.global::<Backend>();
        backend.set_provider_remove_id(id);
        backend.set_provider_remove_name(format!("{} · {}", p.name, p.kind.label()).into());
        backend.set_provider_remove_tags(p.tags.len() as i32);
        self.open_dialog(ui, "provider-remove");
    }

    pub fn confirm_provider_remove(&mut self, ui: &AppWindow, id: i32) {
        let id = i64::from(id);
        self.mutate(
            ui,
            "provider.delete",
            &format!("remove provider #{id}"),
            "Fornecedor removido. Os nomes no extrato foram mantidos como etiquetas.",
            move |conn, actor, _| providers::delete(conn, actor, id),
            move |()| format!("removed provider #{id}; its tags were kept"),
        );
    }

    /// `tag_id` of -1 adds a name to the provider.
    pub fn open_provider_tag(&mut self, ui: &AppWindow, provider_id: i32, tag_id: i32) {
        let Some(provider) = providers::list(&self.conn)
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.id == i64::from(provider_id))
        else {
            return;
        };
        let tag = provider.tags.iter().find(|t| t.id == i64::from(tag_id));
        if tag_id >= 0 && tag.is_none() {
            return;
        }
        ui.global::<Backend>()
            .set_provider_tag_draft(ProviderTagDraft {
                id: tag_id.max(-1),
                provider_id,
                provider_name: provider.name.clone().into(),
                pattern: tag.map(|t| t.pattern.clone()).unwrap_or_default().into(),
                match_index: tag
                    .and_then(|t| MATCH_TYPES.iter().position(|m| *m == t.match_type))
                    .unwrap_or(1) as i32,
                category_index: tag
                    .and_then(|t| CATEGORIES.iter().position(|c| *c == t.category))
                    .unwrap_or(0) as i32,
            });
        self.open_dialog(ui, "provider-tag");
    }

    pub fn save_provider_tag(
        &mut self,
        ui: &AppWindow,
        (provider_id, tag_id, pattern, matching, category): (i32, i32, &str, i32, i32),
    ) {
        let provider = i64::from(provider_id);
        let existing = (tag_id >= 0).then_some(i64::from(tag_id));
        let input = ProviderTagInput {
            pattern: pattern.to_owned(),
            match_type: MATCH_TYPES
                .get(matching.max(0) as usize)
                .copied()
                .unwrap_or(MatchType::Contains),
            category: CATEGORIES
                .get(category.max(0) as usize)
                .copied()
                .unwrap_or(TagCategory::Recurring),
        };
        let note = format!(
            "{} match on {}, category {}",
            input.match_type.as_str(),
            quote(pattern.trim(), 500),
            input.category.as_str()
        );
        self.mutate(
            ui,
            "provider.tag.save",
            &format!("save a statement name of provider #{provider}"),
            "Nome no extrato guardado.",
            move |conn, actor, _| {
                providers::save_tag(conn, actor, provider, &input, existing).map_err(|error| {
                    match error {
                        ServiceError::Duplicate(_) => ServiceError::Rejected(
                            "Já existe uma etiqueta com este nome de correspondência.".into(),
                        ),
                        other => other,
                    }
                })
            },
            move |tag| format!("saved tag #{tag} of provider #{provider} ({note})"),
        );
    }
}

/// Registers the provider callbacks of the window.
pub fn attach(ui: &AppWindow, controller: &Rc<RefCell<Controller>>) {
    let backend = ui.global::<Backend>();
    macro_rules! on {
        ($setter:ident, |$c:ident, $ui:ident $(, $arg:ident)*| $body:expr) => {{
            let (controller, weak) = (controller.clone(), ui.as_weak());
            backend.$setter(move |$($arg),*| {
                let $ui = weak.upgrade().expect("window outlives its callbacks");
                #[allow(unused_mut)]
                let mut $c = controller.borrow_mut();
                $body
            });
        }};
    }
    on!(on_open_provider, |c, ui, id| c.open_provider(&ui, id));
    on!(on_save_provider, |c,
                           ui,
                           id,
                           kind,
                           name,
                           contact,
                           email,
                           phone,
                           iban,
                           note| c
        .save_provider(
            &ui,
            id,
            kind,
            name.as_str(),
            contact.as_str(),
            email.as_str(),
            phone.as_str(),
            iban.as_str(),
            note.as_str()
        ));
    on!(on_open_provider_remove, |c, ui, id| c
        .open_provider_remove(&ui, id));
    on!(on_confirm_provider_remove, |c, ui, id| c
        .confirm_provider_remove(&ui, id));
    on!(on_open_provider_tag, |c, ui, provider, tag| c
        .open_provider_tag(&ui, provider, tag));
    on!(
        on_save_provider_tag,
        |c, ui, provider, tag, pattern, matching, category| c
            .save_provider_tag(&ui, (provider, tag, pattern.as_str(), matching, category))
    );
}
