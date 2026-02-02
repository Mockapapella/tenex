SHELL := bash
.ONESHELL:
.SHELLFLAGS := -euo pipefail -c

BRANCH ?= master
REMOTE ?= origin
VERSION ?= $(shell awk -F'"' '/^version =/ {print $$2; exit}' Cargo.toml)
TAG = v$(VERSION)

.PHONY: help release-prep release-notes release-check release-tag release-publish release-gh release

help:
	@printf '%s\n' \
		'Targets:' \
		'  make release-prep              Checkout $(BRANCH) and pull --ff-only' \
		'  make release-check             Verify version + changelog + cargo publish --dry-run' \
		'  make release-tag               Create and push annotated tag $(TAG)' \
		'  make release-publish CONFIRM_PUBLISH=1   Publish to crates.io' \
		'  make release-gh CONFIRM_GH_RELEASE=1     Create GitHub release from CHANGELOG' \
		'  make release CONFIRM_PUBLISH=1 CONFIRM_GH_RELEASE=1   Run all steps' \
		'' \
		'Variables:' \
		'  VERSION=1.0.8 BRANCH=master REMOTE=origin'

release-prep:
	if [[ -n "$$(git status --porcelain=v1)" ]]; then
		echo "ERROR: working tree not clean" >&2
		git status --porcelain=v1
		exit 1
	fi
	git checkout "$(BRANCH)"
	git pull --ff-only "$(REMOTE)" "$(BRANCH)"

release-check:
	if [[ -n "$$(git status --porcelain=v1)" ]]; then
		echo "ERROR: working tree not clean" >&2
		git status --porcelain=v1
		exit 1
	fi
	current_branch="$$(git branch --show-current)"
	if [[ "$$current_branch" != "$(BRANCH)" ]]; then
		echo "ERROR: expected branch $(BRANCH), got $$current_branch" >&2
		exit 1
	fi
	cargo_version="$$(awk -F'\"' '/^version =/ {print $$2; exit}' Cargo.toml)"
	if [[ "$$cargo_version" != "$(VERSION)" ]]; then
		echo "ERROR: VERSION mismatch (Cargo.toml=$$cargo_version, make VERSION=$(VERSION))" >&2
		exit 1
	fi
	grep -qE "^## \\[$(VERSION)\\]" CHANGELOG.md
	cargo publish --dry-run --locked

release-notes:
	awk -v v="$(VERSION)" '\
		$$0 ~ "^## \\[" v "\\]" {p=1; next} \
		p && $$0 ~ "^## \\[" {exit} \
		p {print} \
	' CHANGELOG.md

release-tag:
	if [[ -n "$$(git status --porcelain=v1)" ]]; then
		echo "ERROR: working tree not clean" >&2
		git status --porcelain=v1
		exit 1
	fi
	current_branch="$$(git branch --show-current)"
	if [[ "$$current_branch" != "$(BRANCH)" ]]; then
		echo "ERROR: expected branch $(BRANCH), got $$current_branch" >&2
		exit 1
	fi
	git fetch "$(REMOTE)" "$(BRANCH)"
	local_head="$$(git rev-parse HEAD)"
	remote_head="$$(git rev-parse "$(REMOTE)/$(BRANCH)")"
	if [[ "$$local_head" != "$$remote_head" ]]; then
		echo "ERROR: $(BRANCH) is not up to date with $(REMOTE)/$(BRANCH)" >&2
		exit 1
	fi
	if git rev-parse -q --verify "refs/tags/$(TAG)" >/dev/null; then
		echo "ERROR: tag $(TAG) already exists" >&2
		exit 1
	fi
	git tag -a "$(TAG)" -m "$(TAG)"
	git push "$(REMOTE)" "$(TAG)"

release-publish:
	if [[ "$${CONFIRM_PUBLISH:-}" != "1" ]]; then
		echo "ERROR: refusing to publish without CONFIRM_PUBLISH=1" >&2
		exit 1
	fi
	cargo publish --locked

release-gh:
	if [[ "$${CONFIRM_GH_RELEASE:-}" != "1" ]]; then
		echo "ERROR: refusing to create a GitHub release without CONFIRM_GH_RELEASE=1" >&2
		exit 1
	fi
	awk -v v="$(VERSION)" '\
		$$0 ~ "^## \\[" v "\\]" {p=1; next} \
		p && $$0 ~ "^## \\[" {exit} \
		p {print} \
	' CHANGELOG.md | gh release create "$(TAG)" --title "$(TAG)" --notes-file -

release: release-prep release-check release-tag release-publish release-gh
