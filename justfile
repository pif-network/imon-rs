cli recipe="ir":
	just -f ./justfile cli_ir

service recipe="dev":
	just -f ./justfile service_dev


cli_ir:
	cargo +nightly install im --path ./cli -Z no-index-update

cli_build:
	cargo +nightly build -p im -Z no-index-update

service_dev:
	cargo watch -s "cargo shuttle run" 
