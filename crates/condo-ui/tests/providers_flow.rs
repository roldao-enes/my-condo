mod common;

use common::{rows, setup_admin, statement_fixture, Harness};
use slint::Model;

const IBAN: &str = "PT50 0002 0123 1234 5678 9015 4";

fn fill_provider(h: &Harness, name: &str, iban: &str) {
    h.type_into("Nome", name);
    h.type_into("Pessoa de contacto", "Joaquim Gonçalves");
    h.type_into("Email", "geral@fornecedor.example.test");
    h.type_into("Telefone", "+351 210 000 001");
    h.type_into("IBAN", iban);
    h.type_into("Nota", "Contrato n.º 123");
}

#[test]
fn registering_a_provider_and_its_statement_names_then_seeing_them_in_finances() {
    let h = Harness::fresh();
    let script = h.script();
    setup_admin(&h);
    h.tall();

    h.click("Fornecedores");
    assert_eq!(h.backend().get_page().as_str(), "providers");
    assert_eq!(h.backend().get_providers().row_count(), 0);
    h.snapshot("40-providers-empty");

    // A bad IBAN is explained in the form and nothing is saved.
    h.click("Novo fornecedor");
    assert_eq!(h.dialog(), "provider");
    fill_provider(&h, "Fornecedor A", "PT50 0002 0123 1234 5678 9015 5");
    h.click("Guardar fornecedor");
    assert_eq!(h.dialog(), "provider");
    assert_eq!(h.error(), "O IBAN indicado não é válido.");

    // Corrected, it is saved, with the IBAN shown in groups of four.
    h.type_into("IBAN", IBAN);
    h.click("Guardar fornecedor");
    assert_eq!(h.dialog(), "", "{}", h.error());
    let all = rows(h.backend().get_providers());
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind.as_str(), "Eletricidade");
    assert_eq!(all[0].iban.as_str(), IBAN);
    assert_eq!(all[0].contact.as_str(), "Joaquim Gonçalves");
    assert_eq!(all[0].note.as_str(), "Contrato n.º 123");
    assert_eq!(
        h.backend().get_providers_subtitle().as_str(),
        "1 fornecedor"
    );

    // Its name in the bank statement.
    h.click("Adicionar nome");
    assert_eq!(h.dialog(), "provider-tag");
    h.type_into("Nome no extrato", "fornecedor a");
    h.click("Guardar nome");
    assert_eq!(h.dialog(), "", "{}", h.error());
    let names = rows(rows(h.backend().get_providers())[0].tags.clone());
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].pattern.as_str(), "FORNECEDOR A");
    h.snapshot("41-providers");

    // Editing keeps what was saved.
    h.click("Editar fornecedor: Fornecedor A");
    assert_eq!(h.value("Telefone"), "+351 210 000 001");
    assert_eq!(h.value("IBAN"), IBAN);
    h.click("Cancelar");

    // The names classify the next statement, and Finanças shows them apart from other tags.
    h.click("Finanças");
    *script.statement.borrow_mut() = Some(statement_fixture());
    h.click("Importar extrato");
    let movements = rows(h.backend().get_fin_tx());
    let theirs: Vec<_> = movements.iter().filter(|t| t.tag_provider).collect();
    assert!(
        !theirs.is_empty(),
        "no movement was matched to the provider"
    );
    assert!(theirs.iter().all(|t| t.tag.as_str() == "Fornecedor A"));
    h.snapshot("42-finances-provider-tag");
    h.click("Etiquetas");
    let tags = rows(h.backend().get_fin_tags());
    assert_eq!(tags[0].provider.as_str(), "Fornecedor A");
    assert_eq!(tags[0].who.as_str(), "Fornecedor");
    h.snapshot("43-finances-tags");

    // Editing a provider's name from Finanças opens it as the provider's own.
    h.click("Editar etiqueta: Fornecedor A");
    assert_eq!(h.dialog(), "provider-tag");
    h.click("Cancelar");

    // Removing the provider keeps the tag as an ordinary one.
    h.click("Fornecedores");
    h.click("Remover fornecedor: Fornecedor A");
    assert_eq!(h.dialog(), "provider-remove");
    h.click("Remover");
    assert_eq!(h.dialog(), "");
    assert_eq!(h.backend().get_providers().row_count(), 0);
    h.click("Finanças");
    h.click("Etiquetas");
    let tags = rows(h.backend().get_fin_tags());
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].provider.as_str(), "");

    // The audit trail records what happened, but not the account number.
    let log = h.audit_log();
    assert!(log.contains("provider.create"));
    assert!(!log.contains("PT50"));
}
