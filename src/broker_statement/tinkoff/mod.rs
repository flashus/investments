mod assets;
mod cash_assets;
mod common;
mod foreign_income;
mod period;
mod securities;
mod trades;

use std::collections::HashMap;
use std::cell::RefCell;
use std::rc::Rc;

use lazy_static::lazy_static;
use regex::{self, Regex};

use crate::broker_statement::dividends::{DividendId, DividendAccruals};
use crate::broker_statement::taxes::TaxAccruals;
#[cfg(test)] use crate::brokers::Broker;
#[cfg(test)] use crate::config::Config;
use crate::core::{GenericResult, EmptyResult};
use crate::formatting;
use crate::instruments::InstrumentId;
#[cfg(test)] use crate::taxes::TaxRemapping;
use crate::xls::{XlsStatementParser, Section, SheetParser, SectionParserRc, Cell};

#[cfg(test)] use super::{BrokerStatement, ReadingStrictness};
use super::{BrokerStatementReader, PartialBrokerStatement};

use assets::AssetsParser;
use cash_assets::CashAssetsParser;
use foreign_income::ForeignIncomeStatementReader;
use period::PeriodParser;
use securities::SecuritiesInfoParser;
use trades::TradesParser;

pub struct StatementReader {
    foreign_income: HashMap<DividendId, (DividendAccruals, TaxAccruals)>
}

impl StatementReader {
    pub fn new() -> GenericResult<Box<dyn BrokerStatementReader>> {
        Ok(Box::new(StatementReader{
            foreign_income: HashMap::new(),
        }))
    }

    // FIXME(konishchev): Validate on close()
    fn parse_foreign_income_statement(&mut self, path: &str) -> EmptyResult {
        for (dividend_id, details) in ForeignIncomeStatementReader::read(path)? {
            if self.foreign_income.insert(dividend_id.clone(), details).is_some() {
                return Err!(
                    "Got a duplicated {}/{} dividend from different foreign income statements",
                    formatting::format_date(dividend_id.date), dividend_id.issuer);
            }
        }

        Ok(())
    }

    fn postprocess(&self, mut statement: PartialBrokerStatement) -> GenericResult<PartialBrokerStatement> {
        let mut dividend_accruals = HashMap::new();
        let mut tax_accruals = HashMap::new();

        for (mut dividend_id, accruals) in statement.dividend_accruals.drain() {
            // FIXME(konishchev): Implement
            // let mut tax_id = TaxId::new(dividend_id.date, dividend_id.issuer);
            // let tax_accruals = statement.tax_accruals.remove(&tax_id);

            let instrument = match dividend_id.issuer {
                InstrumentId::Name(_) => statement.instrument_info.get_or_add_by_id(&dividend_id.issuer)?,
                _ => unreachable!(),
            };

            // FIXME(konishchev): Implement
            // let (accruals, _tax_accruals) = match_statement_dividends_to_foreign_income_data(
            //     &dividend_id, instrument, accruals, tax_accruals, foreign_income)?;

            dividend_id.issuer = InstrumentId::Symbol(instrument.symbol.clone());
            assert!(dividend_accruals.insert(dividend_id, accruals).is_none());
        }

        for (mut tax_id, accruals) in statement.tax_accruals.drain() {
            let instrument = match tax_id.issuer {
                InstrumentId::Name(_) => statement.instrument_info.get_or_add_by_id(&tax_id.issuer)?,
                _ => unreachable!(),
            };

            tax_id.issuer = InstrumentId::Symbol(instrument.symbol.clone());
            assert!(tax_accruals.insert(tax_id, accruals).is_none());
        }

        statement.dividend_accruals = dividend_accruals;
        statement.tax_accruals = tax_accruals;
        statement.validate()
    }
}

impl BrokerStatementReader for StatementReader {
    fn check(&mut self, path: &str) -> GenericResult<bool> {
        let is_foreign_income_statement = ForeignIncomeStatementReader::is_statement(path).map_err(|e| format!(
            "Error while reading {:?}: {}", path, e))?;

        if is_foreign_income_statement {
            self.parse_foreign_income_statement(path).map_err(|e| format!(
                "Error while reading {:?} foreign income statement: {}", path, e))?;
            return Ok(false);
        }

        Ok(path.ends_with(".xlsx"))
    }

    fn read(&mut self, path: &str, _is_last: bool) -> GenericResult<PartialBrokerStatement> {
        let parser = Box::new(StatementSheetParser{});
        let statement = PartialBrokerStatement::new_rc(true);

        let period_parser: SectionParserRc = Rc::new(RefCell::new(
            PeriodParser::new(statement.clone())));

        let trades_parser: SectionParserRc = Rc::new(RefCell::new(
            TradesParser::new(statement.clone())));

        XlsStatementParser::read(path, parser, vec![
            Section::new(PeriodParser::CALCULATION_DATE_PREFIX).by_prefix()
                .parser_rc(period_parser.clone()).required(),
            Section::new(PeriodParser::PERIOD_PREFIX).by_prefix()
                .parser_rc(period_parser).required(),
            Section::new("1.1 Информация о совершенных и исполненных сделках на конец отчетного периода")
                .parser_rc(trades_parser.clone()).required(),
            Section::new("1.2 Информация о неисполненных сделках на конец отчетного периода")
                .parser_rc(trades_parser).required(),
            Section::new("2. Операции с денежными средствами")
                .parser(CashAssetsParser::new(statement.clone())).required(),
            Section::new("3.1 Движение по ценным бумагам инвестора")
                .alias("3. Движение финансовых активов инвестора")
                .parser(AssetsParser::new(statement.clone())).required(),
            Section::new("4.1 Информация о ценных бумагах")
                .parser(SecuritiesInfoParser::new(statement.clone())).required(),
        ])?;

        self.postprocess(Rc::try_unwrap(statement).ok().unwrap().into_inner())
    }
}

struct StatementSheetParser {
}

impl SheetParser for StatementSheetParser {
    fn sheet_name(&self) -> &str {
        "broker_rep"
    }

    fn repeatable_table_column_titles(&self) -> bool {
        true
    }

    fn skip_row(&self, row: &[Cell]) -> bool {
        lazy_static! {
            static ref CURRENT_PAGE_REGEX: Regex = Regex::new(r"^\d+ из$").unwrap();
        }

        enum State {
            None,
            CurrentPage,
            TotalPages,
        }
        let mut state = State::None;

        for cell in row {
            match cell {
                Cell::Empty => {},
                Cell::String(value) => {
                    if !matches!(state, State::None) || !CURRENT_PAGE_REGEX.is_match(value.trim()) {
                        return false;
                    }
                    state = State::CurrentPage;
                }
                Cell::Float(_) | Cell::Int(_) => {
                    if !matches!(state, State::CurrentPage) {
                        return false;
                    }
                    state = State::TotalPages;
                }
                _ => return false,
            };
        }

        matches!(state, State::TotalPages)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use super::*;

    #[test]
    fn parse_real() {
        let statement = parse("main", "my");

        assert!(!statement.cash_assets.is_empty());
        assert!(!statement.deposits_and_withdrawals.is_empty());

        assert!(!statement.fees.is_empty());
        assert!(statement.idle_cash_interest.is_empty());
        assert!(!statement.tax_agent_withholdings.is_empty());

        assert!(!statement.forex_trades.is_empty());
        assert!(!statement.stock_buys.is_empty());
        assert!(!statement.stock_sells.is_empty());
        assert!(statement.dividends.is_empty());

        assert!(!statement.open_positions.is_empty());
        assert!(!statement.instrument_info.is_empty());
    }

    #[rstest(name => ["complex", "mixed-currency-trade"])]
    fn parse_real_other(name: &str) {
        let statement = parse("other", name);
        assert_eq!(!statement.dividends.is_empty(), name == "complex");
    }

    fn parse(namespace: &str, name: &str) -> BrokerStatement {
        let portfolio_name = match (namespace, name) {
            ("main", "my") => s!("tinkoff"),
            ("other", name) => format!("tinkoff-{}", name),
            _ => name.to_owned(),
        };

        let broker = Broker::Tinkoff.get_info(&Config::mock(), None).unwrap();
        let config = Config::load(&format!("testdata/configs/{}/config.yaml", namespace)).unwrap();
        let portfolio = config.get_portfolio(&portfolio_name).unwrap();

        BrokerStatement::read(
            broker, &format!("testdata/tinkoff/{}", name),
            &Default::default(), &Default::default(), &Default::default(),
            TaxRemapping::new(), &portfolio.corporate_actions, ReadingStrictness::all(),
        ).unwrap()
    }
}