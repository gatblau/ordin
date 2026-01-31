//! Unit tests of the File Service: X1 (`error_table`), X2 (`acceptance`, `moving`), and the
//! behaviour behind them (`behaviour`, `workfolder`). They run on SQLite in a temporary folder
//! with the real Journal, Rules Engine and Agent Folder Manager; nothing touches the network or
//! the system bin.

mod acceptance;
mod behaviour;
mod error_table;
mod moving;
mod support;
mod workfolder;
