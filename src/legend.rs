use ratatui::{
    layout::Alignment,
    style::{Style, Stylize},
    text::{Line, Span},
};

pub struct Element<'a> {
    pub name: &'a str,
    pub desc: &'a str,
}

impl<'a> Element<'a> {
    pub fn new(name: &'a str, description: &'a str) -> Self {
        Self {
            name,
            desc: description,
        }
    }
}

/// Private module to house the LegendElement trait.
/// Othrwise the compiler complains about #[warn(private_bounds)]
mod private {
    pub trait LegendElement {
        fn name(&self) -> &str;
        fn desc(&self) -> &str;
    }
}

use private::LegendElement;

pub trait LegendIterator<'a>: Iterator
where
    Self::Item: LegendElement,
{
    fn render(&mut self) -> Line<'a>;
}

impl<'a> LegendIterator<'a> for std::slice::Iter<'a, Element<'a>> {
    fn render(&mut self) -> Line<'a> {
        let mut elements = vec![];
        for entry in self {
            elements.push(Span::styled(
                format!(" {} ", entry.name()),
                Style::new().on_white().black(),
            ));
            elements.push(Span::styled(
                format!(" {} ", entry.desc()),
                Style::new().dark_gray(),
            ));
        }
        Line::from(elements).alignment(Alignment::Center)
    }
}

impl<'a> LegendElement for &'a Element<'a> {
    fn name(&self) -> &str {
        self.name
    }

    fn desc(&self) -> &str {
        self.desc
    }
}
