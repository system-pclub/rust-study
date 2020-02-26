// Copyright 2015-2019 Parity Technologies (UK) Ltd.
// This file is part of Parity Ethereum.

// Parity Ethereum is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity Ethereum is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity Ethereum.  If not, see <http://www.gnu.org/licenses/>.

extern crate ethereum_types;
extern crate futures;
extern crate rpassword;

extern crate parity_rpc as rpc;
extern crate parity_rpc_client as client;

use ethereum_types::U256;
use rpc::signer::ConfirmationRequest;
use client::signer_client::SignerRpc;
use std::io::{Write, BufRead, BufReader, stdout, stdin};
use std::path::PathBuf;
use std::fs::File;

use futures::Future;

fn sign_interactive(
	signer: &mut SignerRpc,
	password: &str,
	request: ConfirmationRequest
) {
	print!("\n{}\nSign this transaction? (y)es/(N)o/(r)eject: ", request);
	let _ = stdout().flush();
	match BufReader::new(stdin()).lines().next() {
		Some(Ok(line)) => {
			match line.to_lowercase().chars().nth(0) {
				Some('y') => {
					match sign_transaction(signer, request.id, password) {
						Ok(s) | Err(s) => println!("{}", s),
					}
				}
				Some('r') => {
					match reject_transaction(signer, request.id) {
						Ok(s) | Err(s) => println!("{}", s),
					}
				}
				_ => ()
			}
		}
		_ => println!("Could not read from stdin")
	}
}

fn sign_transactions(
	signer: &mut SignerRpc,
	password: String
) -> Result<String, String> {
	signer.requests_to_confirm().map(|reqs| {
		match reqs {
			Ok(ref reqs) if reqs.is_empty() => {
				Ok("No transactions in signing queue".to_owned())
			}
			Ok(reqs) => {
				for r in reqs {
					sign_interactive(signer, &password, r)
				}
				Ok("".to_owned())
			}
			Err(err) => {
				Err(format!("error: {:?}", err))
			}
		}
	}).map_err(|err| {
		format!("{:?}", err)
	}).wait()?
}

fn list_transactions(signer: &mut SignerRpc) -> Result<String, String> {
	signer.requests_to_confirm().map(|reqs| {
		match reqs {
			Ok(ref reqs) if reqs.is_empty() => {
				Ok("No transactions in signing queue".to_owned())
			}
			Ok(ref reqs) => {
				Ok(format!("Transaction queue:\n{}", reqs
						   .iter()
						   .map(|r| format!("{}", r))
						   .collect::<Vec<String>>()
						   .join("\n")))
			}
			Err(err) => {
				Err(format!("error: {:?}", err))
			}
		}
	}).map_err(|err| {
		format!("{:?}", err)
	}).wait()?
}

fn sign_transaction(
	signer: &mut SignerRpc, id: U256, password: &str
) -> Result<String, String> {
	signer.confirm_request(id, None, None, None, password).map(|res| {
		match res {
			Ok(u) => Ok(format!("Signed transaction id: {:#x}", u)),
			Err(e) => Err(format!("{:?}", e)),
		}
	}).map_err(|err| {
		format!("{:?}", err)
	}).wait()?
}

fn reject_transaction(
	signer: &mut SignerRpc, id: U256) -> Result<String, String>
{
	signer.reject_request(id).map(|res| {
		match res {
			Ok(true) => Ok(format!("Rejected transaction id {:#x}", id)),
			Ok(false) => Err(format!("No such request")),
			Err(e) => Err(format!("{:?}", e)),
		}
	}).map_err(|err| {
		format!("{:?}", err)
	}).wait()?
}

// cmds

pub fn signer_list(
	signerport: u16, authfile: PathBuf
) -> Result<String, String> {
	let addr = &format!("ws://127.0.0.1:{}", signerport);
	let mut signer = SignerRpc::new(addr, &authfile).map_err(|err| {
		format!("{:?}", err)
	})?;
	list_transactions(&mut signer)
}

pub fn signer_reject(
	id: Option<usize>, signerport: u16, authfile: PathBuf
) -> Result<String, String> {
	let id = id.ok_or(format!("id required for signer reject"))?;
	let addr = &format!("ws://127.0.0.1:{}", signerport);
	let mut signer = SignerRpc::new(addr, &authfile).map_err(|err| {
		format!("{:?}", err)
	})?;
	reject_transaction(&mut signer, U256::from(id))
}

pub fn signer_sign(
	id: Option<usize>,
	pwfile: Option<PathBuf>,
	signerport: u16,
	authfile: PathBuf
) -> Result<String, String> {
	let password;
	match pwfile {
		Some(pwfile) => {
			match File::open(pwfile) {
				Ok(fd) => {
					match BufReader::new(fd).lines().next() {
						Some(Ok(line)) => password = line,
						_ => return Err(format!("No password in file"))
					}
				},
				Err(e) =>
					return Err(format!("Could not open password file: {}", e))
			}
		}
		None => {
			password = match rpassword::prompt_password_stdout("Password: ") {
				Ok(p) => p,
				Err(e) => return Err(format!("{}", e)),
			}
		}
	}

	let addr = &format!("ws://127.0.0.1:{}", signerport);
	let mut signer = SignerRpc::new(addr, &authfile).map_err(|err| {
		format!("{:?}", err)
	})?;

	match id {
		Some(id) => {
			sign_transaction(&mut signer, U256::from(id), &password)
		},
		None => {
			sign_transactions(&mut signer, password)
		}
	}
}
