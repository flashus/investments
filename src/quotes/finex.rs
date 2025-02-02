use std::collections::HashSet;
#[cfg(test)] use std::fs;
use std::io::Cursor;
#[cfg(test)] use std::path::Path;

use calamine::{Reader, Xlsx};
#[cfg(test)] use mockito::{self, Mock, mock};
use reqwest::blocking::{Client, Response};

use xls_table_derive::XlsTableRow;

use crate::core::GenericResult;
#[cfg(test)] use crate::currency::Cash;
use crate::exchanges::Exchange;
use crate::types::Decimal;
use crate::util::{self, DecimalRestrictions};
use crate::xls::{self, SheetReader, SheetParser, TableReader};

use super::{QuotesMap, QuotesProvider};
use super::common::send_request;

// Temporary provider to workaround FinEx funds suspension status (see https://finex-etf.ru/calc/nav)
pub struct Finex {
    client: Client,
}

impl Finex {
    pub fn new() -> Finex {
        Finex {
            client: Client::new(),
        }
    }
}

impl QuotesProvider for Finex {
    fn name(&self) -> &'static str {
        "FinEx"
    }

    fn supports_stocks(&self) -> Option<Exchange> {
        Some(Exchange::Moex)
    }

    fn get_quotes(&self, symbols: &[&str]) -> GenericResult<QuotesMap> {
        #[cfg(not(test))] let base_url = "https://api.finex-etf.ru";
        #[cfg(test)] let base_url = mockito::server_url();

        let url = format!("{}/v1/fonds/nav.xlsx", base_url);
        Ok(send_request(&self.client, &url)
            .and_then(|response| get_quotes(response, symbols))
            .map_err(|e| format!("Failed to get quotes from {}: {}", url, e))?)
    }
}

struct QuotesParser {
}

impl SheetParser for QuotesParser {
    fn sheet_name(&self) -> &str {
        "Report"
    }
}

#[derive(XlsTableRow)]
struct QuotesRow {
    #[column(name="ticker")]
    symbol: String,
    #[column(name="date")]
    _date: String,
    #[column(name="currency")]
    currency: String,
    #[column(name="value")]
    price: Decimal,
}

impl TableReader for QuotesRow {
}

fn get_quotes(response: Response, symbols: &[&str]) -> GenericResult<QuotesMap> {
    let data = response.bytes()?;

    let parser = Box::new(QuotesParser {});
    let sheet_name = parser.sheet_name();

    let sheet = Xlsx::new(Cursor::new(data))?
        .worksheet_range(sheet_name).transpose()?
        .ok_or_else(|| format!("There is no {:?} sheet in the workbook", sheet_name))?;
    let mut reader = SheetReader::new(sheet, parser);

    let mut quotes = QuotesMap::new();
    let symbols: HashSet<&str> = HashSet::from_iter(symbols.iter().copied());

    for quote in xls::read_table::<QuotesRow>(&mut reader)? {
        if !symbols.contains(quote.symbol.as_str()) {
            continue;
        }

        let price = util::validate_named_cash(
            "price", &quote.currency, quote.price,
            DecimalRestrictions::StrictlyPositive)?;

        quotes.insert(quote.symbol, price);
    }

    Ok(quotes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes() {
        let client = Finex::new();
        let _mock = mock_response();

        let mut quotes = QuotesMap::new();
        quotes.insert(s!("FXUS"), Cash::new("USD", dec!(0.721485)));
        quotes.insert(s!("FXDE"), Cash::new("EUR", dec!(0.305032)));
        assert_eq!(client.get_quotes(&["FXUS", "FXDE", "UNKNOWN"]).unwrap(), quotes);
    }

    fn mock_response() -> Mock {
        let path = Path::new(file!()).parent().unwrap().join("testdata/finex.xlsx");

        mock("GET", "/v1/fonds/nav.xlsx")
            .with_status(200)
            .with_header("Content-Type", "application/xlsx; charset=utf-8")
            .with_body(fs::read(path).unwrap())
            .create()
    }
}