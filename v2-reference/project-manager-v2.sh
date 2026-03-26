#!/bin/bash
# project-manager v2 — Research project management with structured R&D objects
# Supports: phases, experiments, decisions, findings (KG), journal, literature
# Storage: JSON DB (project.json) + markdown views for human readability
#
# Usage:
#   project-manager phase add "Phase 5: VKQ PTX" --depends 4
#   project-manager exp add "PTX benchmark" --phase 4 --result pass
#   project-manager finding add "QPs are redundant" --experiment 16
#   project-manager scaffold 4        # decompose phase into task tracker items
#   project-manager journal "entry"   # legacy append-to-journal
#   project-manager status            # overview

set -uo pipefail

# --- Project Selection ---
DEFAULT_PROJECT="volta-renaissance"
PROJECT_NAME="${RESEARCH_PROJECT:-$DEFAULT_PROJECT}"


if [ "${1:-}" = "--project" ] || [ "${1:-}" = "-p" ]; then
    PROJECT_NAME="$2"
    shift 2
elif [ $# -ge 2 ]; then
    # Support: pm <alias> <command> — if first arg looks like a project name
    case "${1:-}" in
        pm-dev|pmdev|vr|volta|tq|turboquant)
            PROJECT_NAME="$1"
            shift
            ;;
    esac
fi

# Resolve short name aliases (AFTER all arg parsing)
case "$PROJECT_NAME" in
    pm-dev|pmdev) PROJECT_NAME="project-manager-dev" ;;
    vr|volta)     PROJECT_NAME="volta-renaissance" ;;
    tq|turboquant) PROJECT_NAME="turboquant" ;;
esac

# Resolve project directory
if [ -n "${PROJECT_DIR:-}" ]; then
    : # use env override
elif [ -d "/home/atari2036/gen-ai/ik_llama.cpp/docs/$PROJECT_NAME" ]; then
    PROJECT_DIR="/home/atari2036/gen-ai/ik_llama.cpp/docs/$PROJECT_NAME"
elif [ "$PROJECT_NAME" = "project-manager-dev" ]; then
    PROJECT_DIR="/usr/local/share/project-manager"
elif [ -d "/home/atari2036/projects/home_cloud/docs/$PROJECT_NAME" ]; then
    PROJECT_DIR="/home/atari2036/projects/home_cloud/docs/$PROJECT_NAME"
else
    PROJECT_DIR="/home/atari2036/gen-ai/ik_llama.cpp/docs/$PROJECT_NAME"
fi

mkdir -p "$PROJECT_DIR"

DB="$PROJECT_DIR/project.json"
JOURNAL="$PROJECT_DIR/research-journal.md"
TS=$(date '+%Y-%m-%d %H:%M')

# --- DB Helpers ---
init_db() {
    if [ ! -f "$DB" ]; then
        cat > "$DB" << DBEOF
{
  "project": "$PROJECT_NAME",
  "created": "$TS",
  "phases": [],
  "experiments": [],
  "decisions": [],
  "findings": [],
  "literature": []
}
DBEOF
    fi
}

next_id() {
    local collection="$1"
    init_db
    local max
    max=$(jq -r ".$collection | map(.id) | max // 0" "$DB")
    echo $((max + 1))
}

# --- Commands ---
cmd_phase() {
    local subcmd="${1:-help}"
    shift || true
    case "$subcmd" in
        add)
            local name="$1"; shift || true
            local depends="" status="pending" impact=0
            while [ $# -gt 0 ]; do
                case "$1" in
                    --depends|--dep) depends="$2"; shift 2 ;;
                    --status) status="$2"; shift 2 ;;
                    --impact) impact="$2"; shift 2 ;;
                    *) shift ;;
                esac
            done
            init_db
            local id=$(next_id phases)
            local deps_json="[]"
            if [ -n "$depends" ]; then
                deps_json="[$(echo "$depends" | tr ',' '\n' | sed 's/^//' | paste -sd,)]"
            fi
            jq --arg name "$name" --arg status "$status" --arg ts "$TS" \
               --argjson id "$id" --argjson deps "$deps_json" --argjson impact "$impact" \
               '.phases += [{"id":$id,"name":$name,"status":$status,"depends_on":$deps,"impact":$impact,"created":$ts}]' \
               "$DB" > "$DB.tmp" && mv "$DB.tmp" "$DB"
            echo "Phase #$id added: $name"
            ;;
        list)
            init_db
            jq -r '.phases[] | "  #\(.id) [\(.status)] \(.name)\(if .depends_on | length > 0 then " (depends: \(.depends_on | join(",")))" else "" end)"' "$DB"
            ;;
        get)
            local id="$1"
            init_db
            jq -r ".phases[] | select(.id == $id)" "$DB"
            ;;
        update)
            local id="$1"; shift
            local status=""
            while [ $# -gt 0 ]; do
                case "$1" in
                    --status) status="$2"; shift 2 ;;
                    *) shift ;;
                esac
            done
            if [ -n "$status" ]; then
                jq --argjson id "$id" --arg status "$status" \
                   '(.phases[] | select(.id == $id)).status = $status' \
                   "$DB" > "$DB.tmp" && mv "$DB.tmp" "$DB"
                echo "Phase #$id updated: status=$status"
            fi
            ;;
        *)
            echo "phase: add|list|get|update"
            ;;
    esac
}

cmd_experiment() {
    local subcmd="${1:-help}"
    shift || true
    case "$subcmd" in
        add)
            local name="$1"; shift || true
            local phase="" result="pending" hypothesis="" status="pending" notes=""
            while [ $# -gt 0 ]; do
                case "$1" in
                    --phase) phase="$2"; shift 2 ;;
                    --result) result="$2"; status="$2"; shift 2 ;;
                    --status) status="$2"; shift 2 ;;
                    --notes) notes="$2"; shift 2 ;;
                    --hypothesis) hypothesis="$2"; shift 2 ;;
                    *) shift ;;
                esac
            done
            # Normalize status to enum
            case "$status" in
                pass|PASS|complete|completed) status="pass" ;;
                fail|FAIL|failed|slower|SLOWER|regression) status="fail" ;;
                pending|Pending) status="pending" ;;
                *) status="inconclusive" ;;
            esac
            init_db
            local id=$(next_id experiments)
            jq --arg name "$name" --arg result "$result" --arg status "$status" --arg ts "$TS" \
               --arg hypothesis "$hypothesis" --arg notes "$notes" \
               --argjson id "$id" --argjson phase "${phase:-null}" \
               '.experiments += [{"id":$id,"name":$name,"phase":$phase,"status":$status,"result":$result,"hypothesis":$hypothesis,"notes":$notes,"date":$ts}]' \
               "$DB" > "$DB.tmp" && mv "$DB.tmp" "$DB"
            echo "Experiment #$id added: $name [$status]"
            ;;
        list)
            local phase_filter=""
            while [ $# -gt 0 ]; do
                case "$1" in
                    --phase) phase_filter="$2"; shift 2 ;;
                    *) shift ;;
                esac
            done
            init_db
            if [ -n "$phase_filter" ]; then
                jq -r --argjson p "$phase_filter" \
                   '.experiments[] | select(.phase == $p) | "  #\(.id) [\(.result)] \(.name) (phase \(.phase))"' "$DB"
            else
                jq -r '.experiments[] | "  #\(.id) [\(.result)] \(.name) (phase \(.phase // "none"))"' "$DB"
            fi
            ;;
        *)
            echo "exp: add|list"
            ;;
    esac
}

cmd_decision() {
    local subcmd="${1:-help}"
    shift || true
    case "$subcmd" in
        add)
            local what="$1"; shift || true
            local why="" experiment=""
            while [ $# -gt 0 ]; do
                case "$1" in
                    --why) why="$2"; shift 2 ;;
                    --experiment|--exp) experiment="$2"; shift 2 ;;
                    *) shift ;;
                esac
            done
            init_db
            local id=$(next_id decisions)
            jq --arg what "$what" --arg why "$why" --arg ts "$TS" \
               --argjson id "$id" --argjson exp "${experiment:-null}" \
               '.decisions += [{"id":$id,"what":$what,"why":$why,"experiment":$exp,"date":$ts}]' \
               "$DB" > "$DB.tmp" && mv "$DB.tmp" "$DB"
            echo "Decision #$id added: $what"
            ;;
        list)
            init_db
            jq -r '.decisions[] | "  #\(.id) \(.what)\(if .experiment then " (exp #\(.experiment))" else "" end)"' "$DB"
            ;;
        *)
            echo "dec: add|list"
            ;;
    esac
}

cmd_finding() {
    local subcmd="${1:-help}"
    shift || true
    case "$subcmd" in
        add)
            local text="$1"; shift || true
            local experiment="" supports="" contradicts=""
            while [ $# -gt 0 ]; do
                case "$1" in
                    --experiment|--exp) experiment="$2"; shift 2 ;;
                    --supports) supports="$2"; shift 2 ;;
                    --contradicts) contradicts="$2"; shift 2 ;;
                    *) shift ;;
                esac
            done
            init_db
            local id=$(next_id findings)
            jq --arg text "$text" --arg ts "$TS" \
               --argjson id "$id" \
               --argjson exp "${experiment:-null}" \
               --argjson sup "${supports:-null}" \
               --argjson contra "${contradicts:-null}" \
               '.findings += [{"id":$id,"text":$text,"experiment":$exp,"supports":$sup,"contradicts":$contra,"date":$ts}]' \
               "$DB" > "$DB.tmp" && mv "$DB.tmp" "$DB"
            echo "Finding #$id added: $text"
            ;;
        list)
            init_db
            jq -r '.findings[] | "  #\(.id) \(.text)\(if .contradicts then " [CONTRADICTS #\(.contradicts)]" else "" end)\(if .supports then " [SUPPORTS #\(.supports)]" else "" end)"' "$DB"
            ;;
        *)
            echo "finding: add|list"
            ;;
    esac
}

cmd_journal() {
    shift || true
    local entry="$*"
    if [ ! -f "$JOURNAL" ]; then
        echo "# $PROJECT_NAME Research Journal" > "$JOURNAL"
        echo "" >> "$JOURNAL"
    fi
    echo "### [$TS]" >> "$JOURNAL"
    echo "$entry" >> "$JOURNAL"
    echo "" >> "$JOURNAL"
    echo "Journal entry added."
}

cmd_status() {
    echo "=== $PROJECT_NAME Status ==="
    init_db
    echo ""
    local p_total p_complete
    p_total=$(jq '.phases | length' "$DB")
    p_complete=$(jq '[.phases[] | select(.status == "complete")] | length' "$DB")
    echo "Phases: $p_complete/$p_total complete"
    jq -r '.phases[] | "  #\(.id) [\(.status)] \(.name)"' "$DB" 2>/dev/null
    echo ""
    local e_total=$(jq '.experiments | length' "$DB")
    echo "Experiments: $e_total"
    local d_total=$(jq '.decisions | length' "$DB")
    echo "Decisions: $d_total"
    local f_total=$(jq '.findings | length' "$DB")
    echo "Findings: $f_total"
    echo ""
    if [ -f "$JOURNAL" ]; then
        echo "Last journal:"
        tail -4 "$JOURNAL" | head -3
    fi
}

cmd_open() {
    echo "=== Open Items ==="
    init_db
    echo "## Pending Phases:"
    jq -r '.phases[] | select(.status != "complete" and .status != "deprioritized") | "  #\(.id) \(.name)"' "$DB"
    echo ""
    echo "## Pending Experiments:"
    jq -r '.experiments[] | select(.result == "pending") | "  #\(.id) \(.name)"' "$DB"
    echo ""
    if [ -f "$JOURNAL" ]; then
        echo "## Questions:"
        grep '?' "$JOURNAL" | grep -v '###' | grep -v '^$' | tail -10
    fi
}

cmd_next() {
    echo "=== Suggested Next Work ==="
    init_db
    echo ""
    echo "## Next Phases (by impact):"
    jq -r '
        .phases as $all |
        [.phases[] | select(.status != "complete" and .status != "deprioritized") |
         select(.depends_on | all(. as $d | $all[] | select(.id == $d) | .status == "complete"))] |
        sort_by(-(.impact // 0)) |
        .[:3][] |
        "  #\(.id) [impact:\(.impact // 0)] \(.name)"
    ' "$DB" 2>/dev/null || echo "  All phases complete or blocked"
    echo ""
    echo "## In-Progress:"
    jq -r '.phases[] | select(.status == "in_progress") | "  #\(.id) [impact:\(.impact // 0)] \(.name)"' "$DB" 2>/dev/null
    echo ""
    # Generate specific executable action
    local in_prog
    in_prog=$(jq -r '[.phases[] | select(.status == "in_progress")] | sort_by(-(.impact // 0)) | first // empty | .id' "$DB" 2>/dev/null)
    local next_phase
    next_phase=$(jq -r '
        .phases as $all |
        [.phases[] | select(.status != "complete" and .status != "deprioritized" and .status != "in_progress") |
         select(.depends_on | all(. as $d | $all[] | select(.id == $d) | .status == "complete"))] |
        sort_by(-(.impact // 0)) | first // empty | .id
    ' "$DB" 2>/dev/null)
    
    if [ -n "$in_prog" ] && [ "$in_prog" != "null" ]; then
        local pname
        pname=$(jq -r --argjson id "$in_prog" '.phases[] | select(.id == $id) | .name' "$DB")
        echo "## ACTION: Phase #$in_prog ($pname) is in-progress. Execute its top pending experiment NOW."
    elif [ -n "$next_phase" ] && [ "$next_phase" != "null" ]; then
        local pname
        pname=$(jq -r --argjson id "$next_phase" '.phases[] | select(.id == $id) | .name' "$DB")
        echo "## ACTION: Start Phase #$next_phase ($pname). Run: pm $PROJECT_NAME phase update $next_phase --status in_progress"
    else
        echo "## ACTION: All phases complete or blocked."
    fi
    echo ""
    echo "## Pending Experiments:"
    jq -r '.experiments[] | select(.result == "pending") | "  #\(.id) \(.name) (phase \(.phase // "none"))"' "$DB"
    echo ""
    if [ -f "$JOURNAL" ]; then
        echo "## Last Journal:"
        tail -4 "$JOURNAL" | head -3
    fi
}

cmd_context() {
    echo "=== $PROJECT_NAME Context ($(date '+%Y-%m-%d')) ==="
    init_db
    echo ""
    echo "## Active Phase:"
    jq -r '.phases[] | select(.status == "in_progress") | "  #\(.id) \(.name)"' "$DB"
    echo ""
    echo "## Recent Experiments (last 3):"
    jq -r '.experiments | sort_by(.date) | last(3) | .[] | "  #\(.id) [\(.result)] \(.name)"' "$DB" 2>/dev/null
    echo ""
    echo "## Recent Decisions (last 3):"
    jq -r '.decisions | sort_by(.date) | last(3) | .[] | "  #\(.id) \(.what)"' "$DB" 2>/dev/null
    echo ""
    echo "## Key Findings:"
    jq -r '.findings[] | "  #\(.id) \(.text)"' "$DB"
}

cmd_commit() {
    shift || true
    local msg="${*:-Auto-save research logs}"
    local gitroot
    gitroot=$(cd "$PROJECT_DIR" && git rev-parse --show-toplevel 2>/dev/null)
    if [ -z "$gitroot" ]; then echo "Not in a git repo"; exit 1; fi
    cd "$gitroot"
    git add "$PROJECT_DIR"/ 2>/dev/null
    if git diff --cached --quiet 2>/dev/null; then
        echo "No changes to commit."
    else
        git commit -m "docs($PROJECT_NAME): $msg

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
        echo "Committed."
    fi
}

# Legacy experiment/decision/paper (positional args)
cmd_legacy_experiment() {
    local name="${2:-unnamed}" config="${3:-}" result="${4:-}" interp="${5:-}"
    init_db
    local id=$(next_id experiments)
    jq --arg name "$name" --arg result "$result" --arg ts "$TS" \
       --arg hypothesis "$config" \
       --argjson id "$id" --argjson phase "null" \
       '.experiments += [{"id":$id,"name":$name,"phase":$phase,"result":$result,"hypothesis":$hypothesis,"date":$ts}]' \
       "$DB" > "$DB.tmp" && mv "$DB.tmp" "$DB"
    echo "Experiment logged: $name"
}

cmd_legacy_decision() {
    local what="${2:-}" why="${3:-}"
    init_db
    local id=$(next_id decisions)
    jq --arg what "$what" --arg why "$why" --arg ts "$TS" \
       --argjson id "$id" --argjson exp "null" \
       '.decisions += [{"id":$id,"what":$what,"why":$why,"experiment":$exp,"date":$ts}]' \
       "$DB" > "$DB.tmp" && mv "$DB.tmp" "$DB"
    echo "Decision logged: $what"
}

cmd_legacy_paper() {
    local ref="${2:-}" title="${3:-}" findings="${4:-}" relevance="${5:-}"
    init_db
    local id=$(next_id literature)
    jq --arg ref "$ref" --arg title "$title" --arg findings "$findings" \
       --arg relevance "$relevance" --arg ts "$TS" --argjson id "$id" \
       '.literature += [{"id":$id,"ref":$ref,"title":$title,"findings":$findings,"relevance":$relevance,"date":$ts}]' \
       "$DB" > "$DB.tmp" && mv "$DB.tmp" "$DB"
    echo "Paper logged: $title"
}


cmd_scaffold() {
    local phase_filter="" FORMAT=""
    while [ $# -gt 0 ]; do
        case "$1" in
            --phase) phase_filter="$2"; shift 2 ;;
            --format) FORMAT="$2"; shift 2 ;;
            *) shift ;;
        esac
    done
    init_db

    echo "=== Scaffold: Phase -> Task Tracker ==="
    echo ""

    if [ -n "$phase_filter" ]; then
        local phase_name
        phase_name=$(jq -r --argjson id "$phase_filter" '.phases[] | select(.id == $id) | .name' "$DB")
        echo "Phase #$phase_filter: $phase_name"

        # Pending experiments as tasks
        jq -r --argjson pid "$phase_filter" --arg pname "$phase_name" '
            .experiments[] | select(.phase == $pid and .result == "pending") |
            "TASK: \(.name) | phase=\($pid) | exp=\(.id)"
        ' "$DB"

        local exp_count
        exp_count=$(jq --argjson pid "$phase_filter" '[.experiments[] | select(.phase == $pid and .result == "pending")] | length' "$DB")
        if [ "$exp_count" = "0" ]; then
            echo "TASK: $phase_name | phase=$phase_filter | exp=none"
        fi
        
        # JSON output for task tracker integration
        if [ "${FORMAT:-}" = "json" ]; then
            echo ""
            echo "--- JSON (for TaskCreate) ---"
            jq --argjson pid "$phase_filter" --arg pname "$phase_name" '[
                .experiments[] | select(.phase == $pid and .result == "pending") |
                {"subject": ("VR Exp #" + (.id|tostring) + ": " + .name),
                 "description": ("Phase " + ($pid|tostring) + " (" + $pname + "): " + .name + " [project-manager experiment #" + (.id|tostring) + "]")}
            ]' "$DB"
        fi
    else
        # All actionable phases sorted by impact
        jq -r '
            .phases as $all |
            [.phases[] | select(.status == "in_progress" or (.status == "pending" and
                (.depends_on | all(. as $d | $all[] | select(.id == $d) | .status == "complete"))))] |
            sort_by(-(.impact // 0))[] |
            "PHASE #\(.id) [impact:\(.impact // 0)] \(.name)"
        ' "$DB" 2>/dev/null

        echo ""
        echo "Use: scaffold --phase <id> for specific phase tasks"
    fi
}


cmd_kg() {
    local subcmd="${1:-help}"
    shift || true
    init_db
    case "$subcmd" in
        traverse)
            local from_type="" from_id=""
            while [ $# -gt 0 ]; do
                case "$1" in
                    --from) 
                        from_type=$(echo "$2" | cut -d: -f1)
                        from_id=$(echo "$2" | cut -d: -f2)
                        shift 2 ;;
                    *) shift ;;
                esac
            done
            if [ -z "$from_type" ]; then
                echo "Usage: kg traverse --from finding:12"
                return
            fi
            echo "=== KG Traversal from $from_type #$from_id ==="
            echo ""
            
            if [ "$from_type" = "finding" ]; then
                # Show the finding
                jq -r --argjson id "$from_id" '.findings[] | select(.id == $id) | "ROOT: Finding #\(.id): \(.text)"' "$DB"
                echo ""
                
                # Find connected experiment
                local exp_id
                exp_id=$(jq -r --argjson id "$from_id" '.findings[] | select(.id == $id) | .experiment // empty' "$DB")
                if [ -n "$exp_id" ] && [ "$exp_id" != "null" ]; then
                    jq -r --argjson eid "$exp_id" '.experiments[] | select(.id == $eid) | "  <- PRODUCED BY Exp #\(.id): \(.name) [\(.result)]"' "$DB"
                fi
                
                # Find findings that support or contradict this one
                jq -r --argjson id "$from_id" '.findings[] | select(.supports == $id) | "  -> SUPPORTED BY Finding #\(.id): \(.text)"' "$DB"
                jq -r --argjson id "$from_id" '.findings[] | select(.contradicts == $id) | "  X CONTRADICTED BY Finding #\(.id): \(.text)"' "$DB"
                
                # Find findings this one supports or contradicts
                local sup con
                sup=$(jq -r --argjson id "$from_id" '.findings[] | select(.id == $id) | .supports // empty' "$DB")
                con=$(jq -r --argjson id "$from_id" '.findings[] | select(.id == $id) | .contradicts // empty' "$DB")
                if [ -n "$sup" ] && [ "$sup" != "null" ]; then
                    jq -r --argjson sid "$sup" '.findings[] | select(.id == $sid) | "  -> SUPPORTS Finding #\(.id): \(.text)"' "$DB"
                fi
                if [ -n "$con" ] && [ "$con" != "null" ]; then
                    jq -r --argjson cid "$con" '.findings[] | select(.id == $cid) | "  X CONTRADICTS Finding #\(.id): \(.text)"' "$DB"
                fi
                
                # Find decisions informed by this finding's experiment
                if [ -n "$exp_id" ] && [ "$exp_id" != "null" ]; then
                    jq -r --argjson eid "$exp_id" '.decisions[] | select(.experiment == $eid) | "  -> INFORMED Decision #\(.id): \(.what)"' "$DB"
                fi
            
            elif [ "$from_type" = "experiment" ]; then
                jq -r --argjson id "$from_id" '.experiments[] | select(.id == $id) | "ROOT: Experiment #\(.id): \(.name) [\(.result)]"' "$DB"
                echo ""
                jq -r --argjson eid "$from_id" '.findings[] | select(.experiment == $eid) | "  -> PRODUCED Finding #\(.id): \(.text)"' "$DB"
                jq -r --argjson eid "$from_id" '.decisions[] | select(.experiment == $eid) | "  -> INFORMED Decision #\(.id): \(.what)"' "$DB"
            
            elif [ "$from_type" = "decision" ]; then
                jq -r --argjson id "$from_id" '.decisions[] | select(.id == $id) | "ROOT: Decision #\(.id): \(.what)\nWhy: \(.why)"' "$DB"
                echo ""
                local dec_exp
                dec_exp=$(jq -r --argjson id "$from_id" '.decisions[] | select(.id == $id) | .experiment // empty' "$DB")
                if [ -n "$dec_exp" ] && [ "$dec_exp" != "null" ]; then
                    jq -r --argjson eid "$dec_exp" '.experiments[] | select(.id == $eid) | "  <- BASED ON Exp #\(.id): \(.name)"' "$DB"
                fi
            fi
            ;;
        
        map)
            echo "=== Knowledge Graph Map ==="
            echo ""
            echo "Nodes:"
            echo "  Findings: $(jq '.findings | length' "$DB")"
            echo "  Experiments: $(jq '.experiments | length' "$DB")"
            echo "  Decisions: $(jq '.decisions | length' "$DB")"
            local total_edges=0
            echo ""
            echo "Edges:"
            jq -r '.findings[] | select(.supports != null) | "  Finding #\(.id) --supports--> Finding #\(.supports)"' "$DB"
            jq -r '.findings[] | select(.contradicts != null) | "  Finding #\(.id) --contradicts--> Finding #\(.contradicts)"' "$DB"
            jq -r '.findings[] | select(.experiment != null and .experiment != 0) | "  Exp #\(.experiment) --produced--> Finding #\(.id)"' "$DB"
            jq -r '.decisions[] | select(.experiment != null) | "  Exp #\(.experiment) --informed--> Decision #\(.id)"' "$DB"
            echo ""
            echo "Clusters (findings grouped by experiment):"
            jq -r '
                [.findings[] | select(.experiment != null and .experiment != 0)] |
                group_by(.experiment) |
                .[] |
                "  Exp #\(.[0].experiment): " + ([.[] | "#\(.id)"] | join(", "))
            ' "$DB" 2>/dev/null
            ;;
        
        cluster)
            echo "=== Finding Clusters ==="
            echo ""
            echo "## By Experiment:"
            /usr/local/bin/pm-kg-cluster "$DB"
            ;;
        
        *)
            echo "kg: traverse --from <type:id> | map"
            echo "  traverse: walk edges from a node"
            echo "  map: show full graph structure"
            ;;
    esac
}


cmd_dashboard() {
    echo "=== Cross-Project Dashboard ==="
    echo ""
    local config="$HOME/.config/project-manager/active-projects.json"
    [ ! -f "$config" ] && { echo "No active projects."; return; }
    
    for pname in $(jq -r '.active[].name' "$config"); do
        local dir=""
        case "$pname" in
            project-manager-dev) dir="/home/atari2036/gen-ai/ik_llama.cpp/docs/project-manager-dev/project.json" ;;
            *) dir="/home/atari2036/gen-ai/ik_llama.cpp/docs/$pname/project.json" ;;
        esac
        [ ! -f "$dir" ] && continue
        
        # Get SINGLE top action: highest-impact in-progress, or highest-impact next
        local top
        top=$(jq -r '
            .phases as $all |
            ([.phases[] | select(.status == "in_progress")] | sort_by(-(.impact // 0)) | first // empty |
             "IN-PROGRESS #\(.id) [impact:\(.impact // 0)] \(.name)") //
            ([.phases[] | select(.status != "complete" and .status != "deprioritized" and .status != "in_progress") |
             select(.depends_on | all(. as $d | $all[] | select(.id == $d) | .status == "complete"))] |
             sort_by(-(.impact // 0)) | first // empty |
             "NEXT #\(.id) [impact:\(.impact // 0)] \(.name)")
        ' "$dir" 2>/dev/null)
        
        [ -n "$top" ] && echo "  [$pname] $top"
    done
    
    echo ""
    echo "## ACTION: Execute the highest-impact item above."
}


cmd_project() {
    local subcmd="${1:-list}"
    shift || true
    local config="$HOME/.config/project-manager/active-projects.json"
    mkdir -p "$(dirname "$config")"
    [ ! -f "$config" ] && echo '{"active":[]}' > "$config"
    
    case "$subcmd" in
        activate|add)
            local name="$1" alias="${2:-$1}"
            if jq -e --arg n "$name" '.active[] | select(.name == $n)' "$config" >/dev/null 2>&1; then
                echo "Already active: $name"
            else
                jq --arg n "$name" --arg a "$alias" '.active += [{"name":$n,"alias":$a}]' "$config" > "$config.tmp" && mv "$config.tmp" "$config"
                echo "Activated: $name (alias: $alias)"
            fi
            ;;
        pause|remove)
            local name="$1"
            jq --arg n "$name" '.active = [.active[] | select(.name != $n)]' "$config" > "$config.tmp" && mv "$config.tmp" "$config"
            echo "Paused: $name"
            ;;
        list)
            echo "Active projects:"
            jq -r '.active[] | "  \(.name) (\(.alias))"' "$config"
            ;;
        *)
            echo "project: activate <name> [alias] | pause <name> | list"
            ;;
    esac
}


cmd_review() {
    init_db
    echo "=== Research Review ==="
    echo ""
    
    # 1. Experiment velocity
    local total_exp=$(jq '.experiments | length' "$DB")
    local completed=$(jq '[.experiments[] | select(.result != "pending")] | length' "$DB")
    local pending=$(jq '[.experiments[] | select(.result == "pending")] | length' "$DB")
    local failed=$(jq '[.experiments[] | select(.result | test("FAIL|slower|SLOWER|regression|0%"))] | length' "$DB" 2>/dev/null || echo 0)
    echo "## Experiment Velocity"
    echo "  Total: $total_exp | Completed: $completed | Pending: $pending | Failed: $failed"
    if [ "$completed" -gt 0 ]; then
        local success_rate=$(( (completed - failed) * 100 / completed ))
        echo "  Success rate: ${success_rate}%"
        if [ "$failed" -gt 3 ] && [ "$success_rate" -lt 30 ]; then
            echo "  WARNING: High failure rate. Consider reviewing approach."
        fi
    fi
    echo ""
    
    # 2. Stagnation detection
    local consecutive_fails=0
    local recent
    recent=$(jq -r '.experiments | sort_by(.date) | last(5) | .[] | (.status // .result)' "$DB" 2>/dev/null)
    for r in $recent; do
        case "$r" in
            fail|inconclusive) consecutive_fails=$((consecutive_fails + 1)) ;;
            *) consecutive_fails=0 ;;
        esac
    done
    echo "## Stagnation Check"
    if [ "$consecutive_fails" -ge 3 ]; then
        echo "  STAGNATION DETECTED: $consecutive_fails consecutive failed/negative experiments."
        echo "  REDIRECT: Current approach is exhausted. Pivot to a different optimization vector."
        echo "  Review findings for alternative approaches."
    else
        echo "  OK: No stagnation detected (consecutive fails: $consecutive_fails)"
    fi
    echo ""
    
    # 3. Phase impact assessment
    echo "## Impact Assessment"
    jq -r '
        .phases[] | select(.status == "in_progress") |
        "  IN-PROGRESS: #\(.id) \(.name) [impact:\(.impact // 0)]"
    ' "$DB"
    local in_prog_impact=$(jq '[.phases[] | select(.status == "in_progress") | .impact // 0] | max // 0' "$DB")
    local max_avail_impact=$(jq '
        .phases as $all |
        [.phases[] | select(.status != "complete" and .status != "deprioritized" and .status != "in_progress") |
         select(.depends_on | all(. as $d | $all[] | select(.id == $d) | .status == "complete")) |
         .impact // 0] | max // 0
    ' "$DB")
    if [ "$max_avail_impact" -gt "$in_prog_impact" ] 2>/dev/null; then
        echo "  WARNING: Higher-impact phase available (impact:$max_avail_impact vs current:$in_prog_impact)"
    fi
    echo ""
    
    # 4. Findings summary
    local total_findings=$(jq '.findings | length' "$DB")
    local contradictions=$(jq '[.findings[] | select(.contradicts != null)] | length' "$DB")
    echo "## Knowledge Graph"
    echo "  Findings: $total_findings | Contradictions: $contradictions"
    if [ "$contradictions" -gt 0 ]; then
        echo "  Active contradictions:"
        jq -r '.findings[] | select(.contradicts != null) | "    Finding #\(.id) contradicts #\(.contradicts): \(.text | .[0:80])..."' "$DB"
    fi
    echo ""
    
    echo "## ACTION: Address any WARNINGS or STAGNATION above before continuing experiments."
}

# --- Main Dispatch ---
case "${1:-help}" in
    phase)    shift; cmd_phase "$@" ;;
    exp|experiment)
        if [ "${2:-}" = "add" ] || [ "${2:-}" = "list" ]; then
            shift; cmd_experiment "$@"
        else
            cmd_legacy_experiment "$@"
        fi
        ;;
    dec|decision)
        if [ "${2:-}" = "add" ] || [ "${2:-}" = "list" ]; then
            shift; cmd_decision "$@"
        else
            cmd_legacy_decision "$@"
        fi
        ;;
    finding)  shift; cmd_finding "$@" ;;
    paper|lit) cmd_legacy_paper "$@" ;;
    journal|j) cmd_journal "$@" ;;
    status|s)  cmd_status ;;
    log|l)
        if [ -f "$JOURNAL" ]; then tail -30 "$JOURNAL"
        else echo "No journal entries yet."; fi
        ;;
    context|ctx) cmd_context ;;
    next|n)    cmd_next ;;
    review|rev) cmd_review ;;
    project|proj) shift; cmd_project "$@" ;;
    dashboard|dash|d) cmd_dashboard ;;
    kg) shift; cmd_kg "$@" ;;
    scaffold|sc) shift; cmd_scaffold "$@" ;;
    commit|c)  cmd_commit "$@" ;;
    open|o)    cmd_open ;;
    help|*)
        echo "project-manager v2 — Research project management with structured R&D objects"
        echo ""
        echo "Structured commands:"
        echo "  phase add <name> [--depends N] [--status S]    Add research phase"
        echo "  phase list                                      List all phases"
        echo "  phase update <id> --status <S>                  Update phase status"
        echo "  exp add <name> [--phase N] [--result S]         Add experiment"
        echo "  exp list [--phase N]                            List experiments"
        echo "  dec add <what> [--why S] [--experiment N]       Add decision"
        echo "  dec list                                        List decisions"
        echo "  finding add <text> [--experiment N]             Add finding (KG node)"
        echo "    [--supports N] [--contradicts N]"
        echo "  finding list                                    List findings"
        echo ""
        echo "Legacy commands:"
        echo "  journal <entry>        Append to research journal"
        echo "  experiment <args>      Log experiment (positional)"
        echo "  decision <args>        Log decision (positional)"
        echo "  paper <args>           Add to literature"
        echo ""
        echo "Workflow:"
        echo "  status                 Project overview"
        echo "  next                   Suggest next work (DAG-aware)"
        echo "  context                Session startup context"
        echo "  open                   Open items and questions"
        echo "  commit [msg]           Git-commit project files"
        echo ""
        echo "Options:"
        echo "  --project <name>       Switch project (default: volta-renaissance)"
        ;;
esac
