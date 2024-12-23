use clap::Parser;
use repo_executor::{api::{Executor, Export, FtpExport}, cli::Arguments};

fn main() {

	let args = Arguments::parse();

	if args.new {

		let result = FtpExport::new(args);

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
	else {

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
}
