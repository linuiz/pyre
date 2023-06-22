use alloc::{boxed::Box, string::String};
use core::ops::Range;
use elf::{endian::AnyEndian, symbol::Symbol};
use libsys::{Address, Virtual};

errorgen! {
    #[derive(Debug)]
    pub enum Error {
        ParserError { err: elf::ParseError } => Some(err),
        NoTables => None
    }
}

pub type SymbolMapping = (Symbol, Option<Range<usize>>);
pub type PackedStrings = String;
pub type PackedSymbolTable = (PackedStrings, Box<[SymbolMapping]>);

static SYMBOLS: spin::Once<PackedSymbolTable> = spin::Once::new();

pub fn parse(kernel_file: &'static limine::File) -> Result<()> {
    SYMBOLS.try_call_once(|| {
        let (symtab, strtab) = elf::ElfBytes::<AnyEndian>::minimal_parse(kernel_file.data())
            .map_err(|err| Error::ParserError { err })?
            .symbol_table()
            .map_err(|err| Error::ParserError { err })?
            .ok_or(Error::NoTables)?;

        let mut strs = alloc::string::String::new();
        let mut symbols = alloc::vec::Vec::with_capacity(symtab.len());
        for symbol in symtab {
            let symbol_substr = strtab
                .get(symbol.st_name as usize)
                .map(|str| {
                    let str_start = strs.len();

                    strs.push_str(str);

                    str_start..strs.len()
                })
                .ok();

            symbols.push((symbol, symbol_substr));
        }

        symbols.shrink_to_fit();
        Ok((strs, symbols.into_boxed_slice()))
    })?;

    Ok(())
}

pub fn get(address: Address<Virtual>) -> Option<(&'static Symbol, Option<&'static str>)> {
    SYMBOLS.get().and_then(|(strs, symbols)| {
        let (symbol, substr) = symbols.iter().find(|(symbol, _)| {
            let symbol_region = symbol.st_value..(symbol.st_value + symbol.st_size);
            symbol_region.contains(&address.get().try_into().unwrap())
        })?;
        let symbol_name = substr.clone().and_then(|substr| strs.get(substr));

        Some((symbol, symbol_name))
    })
}
