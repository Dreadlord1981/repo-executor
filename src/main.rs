use clap::Parser;
use repo_executor::{api::{Executor, Export}, cli::Arguments};

fn main() {

	let args = Arguments::parse();

	let result = Export::new(args);

	match result {

		Ok(exporter) => {

			println!();
			println!("*******************");
			println!("*                 *");
			println!("*  Repo executor  *");
			println!("*                 *");
			println!("*******************");

			let result = exporter.execute();

			if let Err(err) = result {
				println!("Err: {}", err);
			}
		},
		Err(err) => {
			println!("Err: {}", err);
		}
	}	
}
