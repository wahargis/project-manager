#!/bin/bash
# pm-kg-cluster: group findings by experiment
DB="$1"
[ ! -f "$DB" ] && exit 1

echo "## By Experiment:"
jq -r "[.findings[] | select(.experiment != null and .experiment != 0)] | group_by(.experiment) | sort_by(-length) | .[] | (\"  Exp #\" + (.[0].experiment|tostring) + \": \" + (length|tostring) + \" findings\"), (.[] | \"    #\" + (.id|tostring) + \": \" + (.text|.[0:80]))" "$DB"
