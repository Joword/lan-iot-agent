# Developer shortcuts. Windows: use the PowerShell equivalents in README.
.PHONY: test smoke up

test:
	cd apps/agent && python -m pytest -q
	cd apps/hub && cargo test

smoke:
	./scripts/smoke.sh

up:
	cd deploy/docker && docker compose up --build -d
	$(MAKE) smoke
