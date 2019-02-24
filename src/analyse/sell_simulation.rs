use prettytable::{Table, Row, Cell};
use prettytable::format::Alignment;

use crate::broker_statement::BrokerStatement;
use crate::broker_statement::trades::StockSell;
use crate::config::PortfolioConfig;
use crate::core::EmptyResult;
use crate::currency::{Cash, MultiCurrencyCashAccount};
use crate::currency::converter::CurrencyConverter;
use crate::formatting;
use crate::localities;
use crate::quotes::Quotes;

pub fn simulate_sell(
    portfolio: &PortfolioConfig, mut statement: BrokerStatement,
    converter: &CurrencyConverter, mut quotes: Quotes,
    positions: &Vec<(String, u32)>,
) -> EmptyResult {
    for (symbol, _) in positions {
        if statement.open_positions.get(symbol).is_none() {
            return Err!("The portfolio has no open {:?} position", symbol);
        }

        quotes.batch(&symbol);
    }

    for (symbol, quantity) in positions {
        statement.emulate_sell(&symbol, *quantity, quotes.get(&symbol)?)?;
    }
    statement.process_trades()?;

    let mut stock_sells = statement.stock_sells.iter()
        .filter(|stock_sell| stock_sell.emulation)
        .cloned().collect::<Vec<_>>();
    assert_eq!(stock_sells.len(), positions.len());

    print_results(stock_sells, converter)
}

fn print_results(stock_sells: Vec<StockSell>, converter: &CurrencyConverter) -> EmptyResult {
    let country = localities::russia();

    let mut total_commissions = MultiCurrencyCashAccount::new();
    let mut total_revenue = MultiCurrencyCashAccount::new();
    let mut total_profit = MultiCurrencyCashAccount::new();
    let mut total_tax_to_pay = MultiCurrencyCashAccount::new();

    let mut table = Table::new();
    let mut fifo_details = Vec::new();

    for trade in stock_sells {
        let details = trade.calculate(&country, &converter).map_err(|e| format!(
            "Failed calculate results of {} selling order: {}", trade.symbol, e))?;

        total_commissions.deposit(trade.commission);
        total_revenue.deposit(details.revenue);
        total_profit.deposit(details.profit);
        total_tax_to_pay.deposit(details.tax_to_pay);

        let mut details_table = Table::new();

        for source in &details.fifo {
            details_table.add_row(Row::new(vec![
                Cell::new_align(&source.quantity.to_string(), Alignment::RIGHT),
                formatting::cash_cell(source.price),
            ]));
        }

        table.add_row(Row::new(vec![
            Cell::new(&trade.symbol),
            Cell::new_align(&trade.quantity.to_string(), Alignment::RIGHT),
            formatting::cash_cell(trade.price),
            formatting::cash_cell(trade.commission),
            formatting::cash_cell(details.revenue),
            formatting::cash_cell(details.profit),
            formatting::cash_cell(details.tax_to_pay),
        ]));

        fifo_details.push((trade.symbol, details_table));
    }

    let mut totals = Vec::new();
    for _ in 0..3 {
        totals.push(Cell::new(""));
    }
    for total in &[total_commissions, total_revenue, total_profit, total_tax_to_pay] {
        let mut assets_iter = total.iter();

        let cell = if assets_iter.len() == 1 {
            let (currency, &amount) = assets_iter.next().unwrap();
            formatting::cash_cell(Cash::new(currency, amount))
        } else {
            Cell::new("")
        };

        totals.push(cell);
    }
    table.add_row(Row::new(totals));

    formatting::print_statement(
        "Sell simulation results",
        vec!["Instrument", "Quantity", "Price", "Commission", "Revenue", "Profit", "Tax to pay"],
        table,
    );

    for (symbol, details_table) in fifo_details {
        formatting::print_statement(
            &format!("FIFO details for {}", symbol),
            vec!["Quantity", "Price"],
            details_table,
        );
    }

    Ok(())
}
