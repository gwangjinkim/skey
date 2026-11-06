PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin

.PHONY: install build dist vet clean

install:
	go install .

build:
	go build -o senv .

vet:
	go vet ./...

# Cross-platform binaries in ./dist/
dist:
	mkdir -p dist
	GOOS=darwin  GOARCH=arm64 go build -o dist/senv-darwin-arm64 .
	GOOS=darwin  GOARCH=amd64 go build -o dist/senv-darwin-amd64 .
	GOOS=linux   GOARCH=amd64 go build -o dist/senv-linux-amd64 .
	GOOS=linux   GOARCH=arm64 go build -o dist/senv-linux-arm64 .
	GOOS=windows GOARCH=amd64 go build -o dist/senv-windows-amd64.exe .

install-local: build
	mkdir -p $(BINDIR)
	cp senv $(BINDIR)/senv

clean:
	rm -rf dist senv
