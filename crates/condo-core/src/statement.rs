//! Bank statement parsing. Ported from `app/finances/domain/statement.ts`.
//!
//! Money is always integer EUR cents; dates are calendar dates with no time zone.

use chrono::NaiveDate;

use crate::DomainError;

const MAX_ROWS: usize = 10_000;
const MAX_DESCRIPTION_CHARS: usize = 500;
const MAX_CENTS: i64 = 2_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementRow {
    pub date: NaiveDate,
    pub value_date: NaiveDate,
    pub description: String,
    /// Negative = expense, positive = income. Never zero.
    pub amount: i64,
    pub balance: i64,
}

/// Parses a Portuguese-style amount (`1.234,56`, `-7,50`, `€ 12`, `3.5`) into cents.
pub fn parse_money(value: &str) -> Result<i64, DomainError> {
    let stripped: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '€')
        .collect();
    let decimal = if stripped.contains(',') {
        stripped.replace('.', "").replacen(',', ".", 1)
    } else {
        stripped
    };

    let (negative, digits) = match decimal.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, decimal.as_str()),
    };
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (digits, ""),
    };
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty()
        || !all_digits(whole)
        || fraction.len() > 2
        || !all_digits(fraction)
        || (digits.contains('.') && fraction.is_empty())
    {
        return Err(DomainError::new("Montante invalido."));
    }

    // Reject early so the integer math below cannot overflow.
    if whole.len() > 12 {
        return Err(DomainError::new("Montante fora do limite."));
    }
    let whole: i64 = whole
        .parse()
        .map_err(|_| DomainError::new("Montante invalido."))?;
    let fraction: i64 = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<i64>().unwrap_or(0) * 10,
        _ => fraction.parse().unwrap_or(0),
    };
    let cents = whole * 100 + fraction;
    if cents > MAX_CENTS {
        return Err(DomainError::new("Montante fora do limite."));
    }
    Ok(if negative { -cents } else { cents })
}

fn is_iso_date(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn is_pt_date(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 10
        && b[2] == b'/'
        && b[5] == b'/'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 2 || i == 5 || c.is_ascii_digit())
}

fn parse_date(value: &str) -> Result<NaiveDate, DomainError> {
    let parsed = if is_iso_date(value) {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
    } else if is_pt_date(value) {
        NaiveDate::parse_from_str(value, "%d/%m/%Y")
    } else {
        return Err(DomainError::new("Data invalida."));
    };
    parsed.map_err(|_| DomainError::new("Data invalida."))
}

/// Tries tab, `;`, then `,`; the first delimiter yielding exactly 5 columns on every row wins.
fn split_rows(content: &str) -> Vec<Vec<String>> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    for delimiter in *b"\t;," {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(delimiter)
            .trim(csv::Trim::All)
            .from_reader(content.as_bytes());
        let candidate: Result<Vec<Vec<String>>, _> = reader
            .records()
            .map(|record| record.map(|r| r.iter().map(str::to_owned).collect()))
            .collect();
        if let Ok(rows) = candidate {
            let rows: Vec<Vec<String>> = rows
                .into_iter()
                .filter(|row| !(row.len() == 1 && row[0].is_empty()))
                .collect();
            if !rows.is_empty() && rows.iter().all(|row| row.len() == 5) {
                return rows;
            }
        }
    }
    Vec::new()
}

fn is_header_label(value: &str) -> bool {
    matches!(
        value.to_lowercase().as_str(),
        "data" | "date" | "booking date"
    )
}

pub fn parse_statement(content: &str) -> Result<Vec<StatementRow>, DomainError> {
    let mut rows = split_rows(content);
    if rows.is_empty() || rows.len() > MAX_ROWS {
        return Err(DomainError::new("Importe entre 1 e 10000 movimentos."));
    }
    if !is_iso_date(&rows[0][0]) && !is_pt_date(&rows[0][0]) {
        let header = rows.remove(0);
        if !is_header_label(&header[0]) {
            return Err(DomainError::new("Cabecalho invalido."));
        }
    }
    if rows.is_empty() {
        return Err(DomainError::new("O ficheiro nao contem movimentos."));
    }

    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            parse_row(row).map_err(|e| DomainError::new(format!("Movimento {}: {}", index + 1, e)))
        })
        .collect()
}

fn parse_row(row: &[String]) -> Result<StatementRow, DomainError> {
    if row.len() != 5 || row[2].is_empty() || row[2].chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(DomainError::new(
            "Esperadas 5 colunas: data, data valor, descricao, montante e saldo.",
        ));
    }
    let amount = parse_money(&row[3])?;
    if amount == 0 {
        return Err(DomainError::new("O montante nao pode ser zero."));
    }
    Ok(StatementRow {
        date: parse_date(&row[0])?,
        value_date: parse_date(&row[1])?,
        description: row[2].clone(),
        amount,
        balance: parse_money(&row[4])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn imports_the_supplied_portuguese_bank_statement_without_losing_cents() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/bank_statement.tsv"
        );
        let content = std::fs::read_to_string(path).expect("fixture");
        let rows = parse_statement(&content).unwrap();
        assert_eq!(rows.len(), 23);
        assert_eq!(rows[0].amount, -750);
        assert_eq!(rows[0].balance, 492_211);
        assert_eq!(rows[12].amount, 21_400);
        assert_eq!(rows.last().unwrap().balance, 491_890);
        assert_eq!(rows.iter().map(|r| r.amount).sum::<i64>(), -1071);
    }

    #[test]
    fn supports_headers_bom_quoted_fields_and_semicolon_exports() {
        let rows = parse_statement(
            "\u{feff}Data;Data valor;Descricao;Montante;Saldo\n03/01/2025;03/01/2025;\"Fornecedor; luz\";-7,50;4.922,11",
        )
        .unwrap();
        assert_eq!(rows[0].date, d(2025, 1, 3));
        assert_eq!(rows[0].description, "Fornecedor; luz");
        assert_eq!(rows[0].amount, -750);
        assert_eq!(rows[0].balance, 492_211);
        assert_eq!(parse_money("117,68").unwrap(), 11_768);
    }

    #[test]
    fn rejects_invalid_dates_amounts_empty_statements_and_malformed_rows() {
        for content in [
            "",
            "2025-02-30\t2025-01-01\tFee\t-1\t2",
            "2025-01-01\t2025-01-01\tFee\t1x\t2",
            "2025-01-01\tFee\t1",
        ] {
            assert!(
                parse_statement(content).is_err(),
                "should reject {content:?}"
            );
        }
        assert!(parse_money("1,001").is_err());
    }

    #[test]
    fn money_edge_cases() {
        assert_eq!(parse_money("€ 12").unwrap(), 1200);
        assert_eq!(parse_money("3.5").unwrap(), 350);
        assert_eq!(parse_money("-1.234,56").unwrap(), -123_456);
        assert_eq!(parse_money("20000000,00").unwrap(), 2_000_000_000);
        assert!(parse_money("20000000,01").is_err());
        assert!(parse_money("").is_err());
        assert!(parse_money("-").is_err());
        assert!(parse_money("1.").is_err());
        assert!(parse_money("+5").is_err());
        assert!(parse_money("99999999999999999999").is_err());
    }

    #[test]
    fn rejects_zero_amount_and_bad_header() {
        assert!(parse_statement("2025-01-01\t2025-01-01\tFee\t0\t2").is_err());
        assert!(
            parse_statement("Foo\tBar\tBaz\tQux\tQuux\n2025-01-01\t2025-01-01\tFee\t-1\t2")
                .is_err()
        );
        assert!(parse_statement("Date\tVal\tDesc\tAmt\tBal").is_err());
    }
}
