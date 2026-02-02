SHELL := bash
.ONESHELL:
.SHELLFLAGS := -euo pipefail -c

BRANCH ?= master
REMOTE ?= origin

.PHONY: help release release-notes
.SILENT: release release-notes

help:
	@printf '%s\n' \
		'Targets:' \
		'  make release        Publish the version in Cargo.toml (tag + crates.io + GitHub release)' \
		'  make release-notes  Print the CHANGELOG notes for the current version' \
		'' \
		'Variables:' \
		'  BRANCH=master REMOTE=origin'

release-notes:
	version="$$(awk -F'\"' '/^version =/ {print $$2; exit}' Cargo.toml)"
	awk -v v="$$version" '\
		$$0 ~ "^## \\[" v "\\]" {p=1; next} \
		p && $$0 ~ "^## \\[" {exit} \
		p {print} \
	' CHANGELOG.md

release:
	if [[ -n "$$(git status --porcelain=v1)" ]]; then
		echo "ERROR: working tree not clean" >&2
		git status --porcelain=v1
		exit 1
	fi

	git checkout "$(BRANCH)"
	git pull --ff-only "$(REMOTE)" "$(BRANCH)"
	git fetch --tags "$(REMOTE)"

	if [[ -n "$$(git status --porcelain=v1)" ]]; then
		echo "ERROR: working tree not clean after pull" >&2
		git status --porcelain=v1
		exit 1
	fi

	current_branch="$$(git branch --show-current)"
	if [[ "$$current_branch" != "$(BRANCH)" ]]; then
		echo "ERROR: expected branch $(BRANCH), got $$current_branch" >&2
		exit 1
	fi

	version="$$(awk -F'\"' '/^version =/ {print $$2; exit}' Cargo.toml)"
	if [[ -z "$$version" ]]; then
		echo "ERROR: failed to read version from Cargo.toml" >&2
		exit 1
	fi

	tag="v$$version"
	commit="$$(git rev-parse --short HEAD)"

	if ! grep -qE "^## \\[$$version\\]" CHANGELOG.md; then
		echo "ERROR: missing CHANGELOG entry for $$version" >&2
		exit 1
	fi

	echo "Running: cargo publish --dry-run --locked"
	cargo publish --dry-run --locked

	if git rev-parse -q --verify "refs/tags/$$tag" >/dev/null; then
		echo "ERROR: tag $$tag already exists locally" >&2
		exit 1
	fi

	if git ls-remote --exit-code --tags "$(REMOTE)" "refs/tags/$$tag" >/dev/null 2>&1; then
		echo "ERROR: tag $$tag already exists on $(REMOTE)" >&2
		exit 1
	fi

	if ! command -v gh >/dev/null 2>&1; then
		echo "ERROR: gh is required to create the GitHub release" >&2
		exit 1
	fi

	if ! gh auth status -h github.com >/dev/null 2>&1; then
		echo "ERROR: gh is not authenticated (run: gh auth login)" >&2
		exit 1
	fi

	if gh release view "$$tag" >/dev/null 2>&1; then
		echo "ERROR: GitHub release $$tag already exists" >&2
		exit 1
	fi

	if [[ ! -t 0 ]]; then
		echo "ERROR: refusing to publish without an interactive terminal" >&2
		exit 1
	fi

	printf '\n%s\n' "Ready to publish Tenex $$version"
	printf '%s\n' "  Branch: $(BRANCH) ($(REMOTE)/$(BRANCH))"
	printf '%s\n' "  Commit: $$commit"
	printf '%s\n' "  Tag:    $$tag"
	printf '\n%s' "Publish to crates.io and create a GitHub release? [y/N] "
	read -r confirm
	if [[ ! "$$confirm" =~ ^([Yy]|[Yy][Ee][Ss])$$ ]]; then
		echo "Aborted."
		exit 0
	fi

	echo "Publishing to crates.io..."
	cargo publish --locked

	echo "Tagging $$tag..."
	git tag -a "$$tag" -m "$$tag"
	git push "$(REMOTE)" "$$tag"

	notes_file="$$(mktemp)"
	trap 'rm -f "$$notes_file"' EXIT
	awk -v v="$$version" '\
		$$0 ~ "^## \\[" v "\\]" {p=1; next} \
		p && $$0 ~ "^## \\[" {exit} \
		p {print} \
	' CHANGELOG.md > "$$notes_file"

	if [[ ! -s "$$notes_file" ]]; then
		echo "ERROR: failed to extract release notes from CHANGELOG.md for $$version" >&2
		exit 1
	fi

	echo "Creating GitHub release $$tag..."
	gh release create "$$tag" --title "$$tag" --notes-file "$$notes_file"
