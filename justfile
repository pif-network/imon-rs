alias c := cli
alias s := service
alias l := libs


cli scmd *args:
	#!/bin/bash
	if [ {{scmd}} = "ir" ]; then 
		cargo +nightly install im --path ./cli -Z no-index-update
	elif [ {{scmd}} = "build" ]; then
		cargo +nightly build -p im -Z no-index-update
	else
		cargo {{scmd}} -p cli {{args}}
	fi

service scmd *args:
	#!/bin/bash
	if [ {{scmd}} = "dev" ]; then
		cargo watch -s "cargo shuttle run" -i cli/
	elif [ {{scmd}} = "stress" ]; then
		oha -n 250 -c 50 -q 4 --latency-correction --disable-keepalive http://localhost:8000/v1/record/all
	else
		cargo {{scmd}} -p service {{args}}
	fi

libs scmd *args:
	cargo {{scmd}} -p libs {{args}}
