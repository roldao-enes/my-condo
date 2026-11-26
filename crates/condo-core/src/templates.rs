//! Ready-made announcement texts for the emails the administrator sends again and again.
//!
//! A template is plain text with `[PLACEHOLDERS]` in capitals for the parts that change each
//! time (dates, amounts, IBAN). They are easy to spot and fill in while drafting, and
//! [`unfilled_placeholders`] lets the app warn before one goes out by mistake.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Template {
    /// Shown in the picker.
    pub name: &'static str,
    pub subject: &'static str,
    pub body: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        name: "Convocatória de assembleia geral",
        subject: "Convocatória: assembleia geral de condóminos - [DATA]",
        body: "Caros condóminos,\n\
\n\
Por este meio são convocados para a assembleia geral de condóminos, que se realizará:\n\
\n\
Data: [DATA]\n\
Hora: [HORA]\n\
Local: [LOCAL]\n\
\n\
Ordem de trabalhos:\n\
1. [ASSUNTO 1]\n\
2. [ASSUNTO 2]\n\
3. Outros assuntos de interesse para o condomínio\n\
\n\
Caso à hora marcada não esteja representada a maioria exigida, a assembleia reunirá em segunda convocatória em [DATA DA SEGUNDA CONVOCATÓRIA], às [HORA DA SEGUNDA CONVOCATÓRIA], no mesmo local.\n\
\n\
Os condóminos que não possam estar presentes podem fazer-se representar por outra pessoa, mediante carta assinada dirigida ao administrador.\n\
\n\
A sua presença é muito importante.\n\
\n\
Com os melhores cumprimentos,\n\
A administração",
    },
    Template {
        name: "Cobrança da quota trimestral",
        subject: "Quota de condomínio - [TRIMESTRE] de [ANO]",
        body: "Caros condóminos,\n\
\n\
Informamos que se encontra em cobrança a quota de condomínio referente ao [TRIMESTRE] de [ANO].\n\
\n\
Valor: [VALOR] por fração\n\
Data limite de pagamento: [DATA LIMITE]\n\
\n\
O pagamento deve ser feito por transferência bancária para:\n\
IBAN: [IBAN]\n\
Titular: [TITULAR]\n\
Referência: indique a sua fração (por exemplo, [FRAÇÃO])\n\
\n\
Se já efetuou o pagamento, agradecemos que desconsidere esta mensagem.\n\
\n\
Para qualquer esclarecimento, responda a este email.\n\
\n\
Com os melhores cumprimentos,\n\
A administração",
    },
];

/// Every `[PLACEHOLDER]` still in `text`, in order and without repeats. Only bracketed text with
/// no lowercase letters counts, so ordinary prose in brackets is left alone.
pub fn unfilled_placeholders(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            break;
        };
        let inner = &after[..close];
        let is_placeholder = !inner.is_empty()
            && !inner.contains('[')
            && inner.chars().any(char::is_alphabetic)
            && !inner.chars().any(char::is_lowercase);
        if is_placeholder {
            let tag = format!("[{inner}]");
            if !found.contains(&tag) {
                found.push(tag);
            }
        }
        rest = &after[close + 1..];
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_a_meeting_and_a_quarterly_fee_template() {
        let names: Vec<_> = TEMPLATES.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "Convocatória de assembleia geral",
                "Cobrança da quota trimestral"
            ]
        );
    }

    #[test]
    fn templates_have_placeholders_to_fill_in_and_no_stray_brackets() {
        for t in TEMPLATES {
            assert!(!unfilled_placeholders(t.subject).is_empty(), "{}", t.name);
            assert!(!unfilled_placeholders(t.body).is_empty(), "{}", t.name);
            for text in [t.subject, t.body] {
                assert_eq!(
                    text.matches('[').count(),
                    text.matches(']').count(),
                    "{}",
                    t.name
                );
            }
        }
    }

    #[test]
    fn every_bracket_in_a_template_is_recognised_as_a_placeholder() {
        for t in TEMPLATES {
            for text in [t.subject, t.body] {
                let all = text.matches('[').count();
                let mut counted = 0;
                for tag in unfilled_placeholders(text) {
                    counted += text.matches(&tag).count();
                }
                assert_eq!(all, counted, "{}", t.name);
            }
        }
    }

    #[test]
    fn finds_each_placeholder_once_in_order() {
        assert_eq!(
            unfilled_placeholders("[DATA] às [HORA], ou [DATA]; veja [nota] e [1]"),
            ["[DATA]", "[HORA]"]
        );
        assert!(unfilled_placeholders("Tudo preenchido, ver [anexo].").is_empty());
        assert!(unfilled_placeholders("aberto [DATA").is_empty());
    }
}
