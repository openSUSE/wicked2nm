#!/bin/bash
RED='\033[0;31m'
GREEN='\033[0;32m'
BOLD='\033[1m'
NC='\033[0m'
FAILED_TESTS=()
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd $SCRIPT_DIR
MIGRATE_WICKED_BIN="$SCRIPT_DIR/../target/debug/wicked2nm"
TEST_DIRS=${TEST_DIRS:-$(ls -d */ | sed 's#/##')}
NO_CLEANUP=${NO_CLEANUP:-0}
NO_WICKED=${NO_WICKED:-0}
NM_CLEANUP=${NM_CLEANUP:-0}
LOG_LEVEL=1
TEST_STDIN=true
NM_VERSION=$(NetworkManager --version)
KEEP_CONNECTIONS=()
CONNECTIONS=()

error_msg() {
  log_error "Error for test $1:$2"
}

log_error() {
  [ $LOG_LEVEL -gt 0 ] && echo -e "${RED}$@${NC}"
}

log_verbose() {
    [ $LOG_LEVEL -gt 1 ] && echo -e "$@"
}

keep_connection() {
    local con=$1
    local element

    for element in "${KEEP_CONNECTIONS[@]}"; do
        if [[ "$element" == "$con" ]]; then
            return 0 # Found
        fi
    done
    return 1 # Not found
}

refresh_connections() {
    local filename 

    CONNECTIONS=()
    while IFS= read -r -d '' filename ; do
        [[ "$filename" != *".nmconnection" ]] && continue
        uuid=$(grep '^uuid=' "$filename" -m 1 | cut -d=  -f2)
        con_name=$(grep '^id=' "$filename" -m 1 | cut -d=  -f2)
        keep_connection "$con_name" && continue
        keep_connection "$uuid" && continue
        CONNECTIONS+=("$con_name")
    done < <(find "/etc/NetworkManager/system-connections/" -maxdepth 1 -type f -print0)
}

nm_cleanup() {
    refresh_connections

    for con in "${CONNECTIONS[@]}"; do
        IFS=: read NAME UUID XXX < <(nmcli -t -f NAME,UUID c s | grep -P '(^|:)'"$con"'($|:)')
        [[ -z "$UUID" ]] && continue;

        echo "nmcli con delete $UUID ($NAME)"
        nmcli con delete "$UUID"
        while nmcli -t -f UUID c s | grep -P "^$UUID$" >/dev/null; do
          echo " -> Wait for $NAME($UUID) deletion"
            sleep 1 
        done
    done
}

nm_version_greater_equal() {
    printf '%s\n%s\n' "$1" "$NM_VERSION" | sort --check=quiet --version-sort
}

print_help()
{
  echo "Usage:"
  echo "   ./test [ARGUMENTS] [TEST_DIRS]"
  echo ""
  echo "Arguments:"
  echo "  -v|--verbose            Be more verbose"
  echo "  --debug                 Prints out all commands executed"
  echo "  -q|--quiet              Be less verbose"
  echo "  --nm-cleanup            Cleanup current NetworkManager config before start"
  echo "  -k|--keep-connection    Connections will not be removed with --nm-cleanup"
  echo "  --no-cleanup            Do not cleanup NetworkManger after test"
  echo "  --no-wicked             If set, ifcfg tests do not fail when wicked isn't available"
  echo "  -h|--help               Print this help"
}

POSITIONAL_ARGS=()

while [[ $# -gt 0 ]]; do
  opt=$1; 
  shift;
  case $opt in
    -v|--verbose)
      LOG_LEVEL=$((LOG_LEVEL + 1))
      ;;
    --debug)
      set -x
      ;;
    --binary)
      MIGRATE_WICKED_BIN=$1; shift
      ;;
    --no-wicked)
      NO_WICKED=1;
      ;;
    -k|--keep-connection)
      KEEP_CONNECTIONS+=("$1")
      shift;
      ;;
    --nm-cleanup)
      NM_CLEANUP=1
      ;;
    --no-cleanup)
      NO_CLEANUP=1
      ;;
    -q|--quiet)
      [ $LOG_LEVEL -gt 0 ] && LOG_LEVEL=$((LOG_LEVEL - 1))
      ;;
    -h|--help)
      print_help
      exit 0;
      ;;
    -*|--*)
      echo "Unknown option $opt"
      exit 1
      ;;
    *)
      POSITIONAL_ARGS+=("$opt") # save positional arg
      ;;
  esac
done

if [ ${#POSITIONAL_ARGS[@]} -gt 0 ]; then
  TEST_STDIN=false
  TEST_DIRS=()
  for pos_arg in "${POSITIONAL_ARGS[@]}"; do
      if [[ "$pos_arg" == "stdin" ]]; then
          TEST_STDIN=true
      else
          TEST_DIRS+=("$pos_arg")
      fi
  done
fi

if [[ $NM_CLEANUP -gt 0 ]]; then
    refresh_connections
    if [ ${#CONNECTIONS[@]} -gt 0 ]; then
        echo -e "The following connections will be deleted:"
        for c in "${CONNECTIONS[@]}"; do echo "  $c"; done | sort
        read -p "Do you want to continue? [y/N] " continue_cleanup
        if [ "$continue_cleanup" != "y" ]; then
            exit 1
        fi
        nm_cleanup
    fi
fi

refresh_connections
if [[ ${#CONNECTIONS[@]} -gt 0 ]]; then
    echo -e "${RED}There are already NM connections. You may be running this script on a live system, which is highly discouraged!${NC}"
    exit 1
fi

if [ ! -f $MIGRATE_WICKED_BIN ]; then
    echo -e "${RED}No wicked2nm binary found${NC}"
    exit 1
fi

for test_dir in ${TEST_DIRS}; do
    if [ ! -d "$SCRIPT_DIR/$test_dir" ]; then
        echo -e "${RED}[ERROR]${NC} Directory ${BOLD}$test_dir${NC} doesn't exists!"
        FAILED_TESTS+=("${test_dir}::test-dir-exists")
        continue
    fi

    echo -e "${BOLD}Testing ${test_dir}${NC}"

    cd $SCRIPT_DIR/$test_dir

    # Apply environment variables of test
    export W2NM_CONTINUE_MIGRATION=false
    export W2NM_WITHOUT_NETCONFIG=true
    export W2NM_NETCONFIG_PATH=
    export W2NM_NETCONFIG_DHCP_PATH=
    NM_VERSION_lt=
    NM_VERSION_ge=
    TEST_EXPECT_FAIL=false
    if [ -f  ./ENV ]; then
       set -a && source ./ENV
       set +a
    fi

    if [ ! -z "$NM_VERSION_ge" ] && ! nm_version_greater_equal "$NM_VERSION_ge"; then
        echo "NM version too low, skipping..."
        continue
    fi
    if [ ! -z "$NM_VERSION_lt" ] && nm_version_greater_equal "$NM_VERSION_lt"; then
        echo "NM version too high, skipping..."
        continue
    fi

    if ls -1 ./netconfig/ifcfg-* >/dev/null 2>&1 && [ $NO_WICKED -eq 0 ]; then
        err_log="$SCRIPT_DIR/$test_dir/wicked_show_config_error.log"
        cfg_out="$SCRIPT_DIR/$test_dir/wicked_xml/config.xml"
        if ! command -v wicked >/dev/null ; then
            error_msg "$test_dir" "missing wicked executable"
            FAILED_TESTS+=("${test_dir}::wicked-show-config")
            continue
        fi

        wicked show-config --ifconfig compat:./netconfig \
            > "$cfg_out" \
            2> "$err_log"

        if [ $? -ne 0 ] || [ -s "$err_log" ]; then
            err_msg="'wicked show-config' failed"
            [ -s "$err_log" ] && err_msg+=" see $err_log"

            error_msg "$test_dir" "$err_msg"
            FAILED_TESTS+=("${test_dir}::wicked-show-config")
            continue
        fi

        # https://unix.stackexchange.com/a/209744
        regex_esc_test_dir="$(printf '%s' "$test_dir" | sed 's/[\/.[\(*^$+?{|]/\\&/g')"
        sed -i -E 's/[^:]+(\/tests\/'"$regex_esc_test_dir"')/\1/' "$cfg_out"
    fi

    log_verbose "RUN: $MIGRATE_WICKED_BIN show $test_dir/wicked_xml"
    $MIGRATE_WICKED_BIN show ./wicked_xml
    if [ $? -ne 0 ] && [ "$TEST_EXPECT_FAIL" = false ]; then
        error_msg ${test_dir} "show failed"
        FAILED_TESTS+=("${test_dir}::show")
    fi

    log_verbose "RUN: $MIGRATE_WICKED_BIN migrate $test_dir/wicked_xml"
    $MIGRATE_WICKED_BIN migrate ./wicked_xml
    if [ $? -ne 0 ] && [ "$TEST_EXPECT_FAIL" = false ]; then
        error_msg ${test_dir} "migration failed"
        FAILED_TESTS+=("${test_dir}::migrate")
        continue
    elif [ $? -ne 0 ] && [ "$TEST_EXPECT_FAIL" = true ]; then
        echo -e "${GREEN}Migration for $test_dir failed as expected${NC}"
    fi

    if [ -d "./system-connections" ]; then
      for cmp_file in $(ls -1 ./system-connections/); do
          a="./system-connections/$cmp_file"
          b="/etc/NetworkManager/system-connections/${cmp_file}"
          diff_cmd="diff --unified=0 --color=always -I uuid -I timestamp $a $b"
          log_verbose "RUN: $diff_cmd"
          if $diff_cmd; then
              echo -e "${GREEN}Migration for connection ${cmp_file/\.nmconnection/} successful${NC}"
          else
              diff_cmd="diff  -I uuid -I timestamp --color=always $a $b"
              log_verbose "RUN: $diff_cmd\n$($diff_cmd)\n"
              error_msg ${test_dir} "$cmp_file didn't match"
              FAILED_TESTS+=("${test_dir}::compare_config::${cmp_file}")
          fi
      done
    fi

    [ "$NO_CLEANUP" -gt 0 ] || nm_cleanup
done

if $TEST_STDIN; then
    echo -e "${BOLD}Testing stdin show${NC}"
    cat <<EOF | $MIGRATE_WICKED_BIN show --without-netconfig - | grep "192.168.100.5" &>/dev/null
<interface>
  <ipv4:static>
    <address>
      <local>192.168.100.5/24</local>
    </address>
  </ipv4:static>
</interface>
EOF
    if [ $? -ne 0 ]; then
        error_msg "stdin" "show failed"
        FAILED_TESTS+=("stdin::show")
    else
        echo -e "${GREEN}stdin show successful${NC}"
    fi

    echo -e "${BOLD}Testing stdin migrate${NC}"
    cat <<EOF | $MIGRATE_WICKED_BIN migrate --without-netconfig --dry-run --log-level DEBUG - 2>&1 | grep "192.168.100.5" &>/dev/null
<interface>
  <ipv4:static>
    <address>
      <local>192.168.100.5/24</local>
    </address>
  </ipv4:static>
</interface>
EOF
    if [ $? -ne 0 ]; then
        error_msg "stdin" "migrate failed"
        FAILED_TESTS+=("stdin::migrate")
    else
        echo -e "${GREEN}stdin migrate successful${NC}"
    fi
fi

echo -e "\n${BOLD}Result:${NC}"

if [ ${#FAILED_TESTS[@]} -eq 0 ]; then
    echo -e "${GREEN}All tests successful${NC}"
else
    echo -e "${RED}Failed test cases:"
    for testcase in "${FAILED_TESTS[@]}"; do
        echo "  $testcase"
    done
    echo -n -e "${NC}"
    exit 1
fi
