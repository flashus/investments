use xls_table_derive::XlsTableRow;

use crate::broker_statement::partial::PartialBrokerStatementRc;
use crate::core::EmptyResult;
use crate::instruments::parse_isin;
use crate::xls::{self, XlsStatementParser, SectionParser, SheetReader, Cell, SkipCell, TableReader};

use super::common::read_next_table_row;

pub struct SecuritiesInfoParser {
    statement: PartialBrokerStatementRc,
}

impl SecuritiesInfoParser {
    pub fn new(statement: PartialBrokerStatementRc) -> Box<dyn SectionParser> {
        Box::new(SecuritiesInfoParser {statement})
    }
}

impl SectionParser for SecuritiesInfoParser {
    fn parse(&mut self, parser: &mut XlsStatementParser) -> EmptyResult {
        let mut statement = self.statement.borrow_mut();

        for security in xls::read_table::<SecuritiesInfoRow>(&mut parser.sheet)? {
            let instrument = statement.instrument_info.get_or_add(&security.symbol);
            instrument.set_name(&security.name);
            instrument.add_isin(parse_isin(&security.isin)?);
        }

        Ok(())
    }
}

#[derive(XlsTableRow)]
struct SecuritiesInfoRow {
    #[column(name="Сокращенное наименование актива")]
    name: String,
    #[column(name="Код актива")]
    symbol: String,
    #[column(name="ISIN")]
    isin: String,
    #[column(name="Код государственной регистрации", alias="Номер гос.регистрации")]
    _3: SkipCell,
    #[column(name="Наименование эмитента")]
    _4: SkipCell,
    #[column(name="Тип")]
    _5: SkipCell,
    #[column(name="Номинал")]
    _6: SkipCell,
    #[column(name="Валюта номинала")]
    _7: SkipCell,
}

impl TableReader for SecuritiesInfoRow {
    fn next_row(sheet: &mut SheetReader) -> Option<&[Cell]> {
        read_next_table_row(sheet)
    }
}