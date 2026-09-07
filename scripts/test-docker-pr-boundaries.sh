#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
workflow=${1:-"$root/.github/workflows/docker.yml"}
ruby - "$workflow" <<'RUBY'
require 'yaml'
workflow = YAML.safe_load(File.read(ARGV.fetch(0)), aliases: true)
fail 'relay default must belong to this repository' unless workflow.fetch('env').fetch('IMAGE_NAME') == "${{ vars.GHCR_IMAGE != '' && vars.GHCR_IMAGE || format('ghcr.io/{0}', github.repository) }}"
fail 'gateway default must belong to this repository' unless workflow.fetch('env').fetch('GATEWAY_IMAGE_NAME') == "${{ vars.GHCR_PUSH_GATEWAY_IMAGE != '' && vars.GHCR_PUSH_GATEWAY_IMAGE || format('ghcr.io/{0}-push-gateway', github.repository) }}"
seen = 0
workflow.fetch('jobs').each_value do |job|
  next unless job.fetch('steps', []).any? { |s| s.fetch('uses', '').start_with?('docker/build-push-action@') }
  job.fetch('steps').each do |step|
    uses = step.fetch('uses', '')
    if uses.start_with?('docker/login-action@')
      fail 'PR must not log in for build/cache publication' unless step['if'] == "github.event_name != 'pull_request'"
    end
    next unless uses.start_with?('docker/build-push-action@')
    seen += 1
    args = step.fetch('with')
    fail 'image output must refuse PR publication' unless args.fetch('outputs').end_with?("push=${{ github.event_name != 'pull_request' }}")
    if args.key?('cache-to')
      value = args.fetch('cache-to').strip
      fail 'nested expression cannot select the cache namespace' unless value.scan('${{').length == 1
      fail 'cache write must exclude every PR, including same-repo PRs' unless value.start_with?("${{ github.event_name != 'pull_request' && format(") && value.end_with?(" || '' }}")
    end
    %w[outputs cache-from cache-to].each do |key|
      fail 'build cannot write/read a hardcoded upstream namespace' if args.fetch(key, '').include?('ghcr.io/block/')
    end
  end
end
fail 'all release/debug/gateway build paths must be inspected' unless seen == 3
puts 'Docker PR registry boundaries passed (3 build paths)'
RUBY
