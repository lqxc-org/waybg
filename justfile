default:
    @just --list

fmt:
    zig build fmt

fmt-check:
    zig build fmt-check

build:
    zig build

run:
    zig build run

test:
    zig build test

check:
    zig build ci

release:
    zig build package -Doptimize=ReleaseSafe --prefix dist

release-version version:
    zig build package -Doptimize=ReleaseSafe --prefix dist -Drelease-version={{version}}

clean:
    rm -rf .zig-cache zig-out dist
