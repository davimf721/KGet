use rustyline::error::ReadlineError;
use rustyline::Editor;

pub fn interactive_mode() {
    let mut rl = Editor::<(), _>::new().unwrap();
    println!("KGet Interactive Mode. Type 'help' for commands, 'exit' to quit.");

    loop {
        let readline = rl.readline("kget> ");
        match readline {
            Ok(line) => {
                let input = line.trim();
                let _ = rl.add_history_entry(input);

                match input {
                    "exit" | "quit" => {
                        println!("Bye!");
                        break;
                    }
                    "help" => {
                        println!("Available commands: help, exit, download <url> [output]");
                    }
                    cmd if cmd.starts_with("download ") => {
                        // Parse and call your download logic here
                        println!("Would download: {}", &cmd["download ".len()..]);
                    }
                    "" => {} // Ignore empty
                    _ => println!("Unknown command: '{}'", input),
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                println!("Bye!");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }
}