#!/bin/bash
RED='\033[0;31m'
GREEN='\033[0;32m'
BOLD='\033[1m'
NC='\033[0m'
FAILED_TESTS=()
MIGRATE_WICKED_BIN=../target/debug/migrate-wicked
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd $SCRIPT_DIR
TEST_DIRS=${TEST_DIRS:-$(ls -d */ | sed 's#/##')}
NO_CLEANUP=${NO_CLEANUP:-0}
LOG_LEVEL=1

error_msg() {
  log_error "Error for test $1:$2"
}

log_error() {
  [ $LOG_LEVEL -gt 0 ] && echo -e "${RED}$@${NC}"
}

log_verbose() {
    [ $LOG_LEVEL -gt 1 ] && echo -e "$@"
}

nm_cleanup() {
    for con in $(ls /etc/NetworkManager/system-connections/ | sed 's/\.nmconnection//'); do
        nmcli con delete $con
    done
}

print_help()
{
  echo "Usage:"
  echo "   ./test [ARGUMENTS] [TEST_DIRS]"
  echo ""
  echo "Arguments:"
  echo "  -v|--verbose            Be more verbose"
  echo "  -q|--quiet              Be less verbose"
  echo "  --nm-cleanup            Cleanup current NetworkManager config before start"
  echo "  --no-cleanup            Do not cleanup NetworkManger after test"
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
    --binary)
      MIGRATE_WICKED_BIN=$1; shift
      ;;
    --nm-cleanup)
      nm_cleanup
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
  TEST_DIRS="${POSITIONAL_ARGS[@]}"
fi

if [[ $(ls -A /etc/NetworkManager/system-connections/) ]]; then
    echo -e "${RED}There are already NM connections. You may be running this script on a live system, which is highly discouraged!${NC}"
    exit 1
fi

if [ ! -f $MIGRATE_WICKED_BIN ]; then
    echo -e "${RED}No migrate-wicked binary found${NC}"
    exit 1
fi

for test_dir in ${TEST_DIRS}; do
    echo -e "${BOLD}Testing ${test_dir}${NC}"

    migrate_args=""
    show_args=""

    if [[ $test_dir == *"failure" ]]; then
        expect_fail=true
    else
        expect_fail=false
        migrate_args+=" -c"
    fi

    if [ -d $test_dir/netconfig ]; then
        migrate_args+=" --netconfig-path $test_dir/netconfig/config"
        show_args+=" --netconfig-path $test_dir/netconfig/config"
    else
        migrate_args+=" --without-netconfig"
        show_args+=" --without-netconfig"
    fi

    log_verbose "RUN: $MIGRATE_WICKED_BIN show $show_args $test_dir/wicked_xml"
    $MIGRATE_WICKED_BIN show $show_args $test_dir/wicked_xml
    if [ $? -ne 0 ] && [ "$expect_fail" = false ]; then
        error_msg ${test_dir} "show failed"
        FAILED_TESTS+=("${test_dir}::show")
    fi

    log_verbose "RUN: $MIGRATE_WICKED_BIN migrate $migrate_args $test_dir/wicked_xml"
    $MIGRATE_WICKED_BIN migrate $migrate_args $test_dir/wicked_xml
    if [ $? -ne 0 ] && [ "$expect_fail" = false ]; then
        error_msg ${test_dir} "migration failed"
        FAILED_TESTS+=("${test_dir}::migrate")
        continue
    elif [ $? -ne 0 ] && [ "$expect_fail" = true ]; then
        echo -e "${GREEN}Migration for $test_dir failed as expected${NC}"
    fi

    for cmp_file in $(ls -1 $test_dir/system-connections/); do
        a="$test_dir/system-connections/$cmp_file"
        b="/etc/NetworkManager/system-connections/${cmp_file}"
        diff_cmd="diff --unified=0 --color=always -I uuid -I timestamp $a $b" 
        log_verbose "RUN: $diff_cmd"
        if $diff_cmd; then
            echo -e "${GREEN}Migration for connection ${cmp_file/\.nmconnection/} successful${NC}"
        else
            diff_cmd="diff  -I uuid -I timestamp -y --color=always $a $b"
            log_verbose "RUN: $diff_cmd\n$($diff_cmd)\n" 
            error_msg ${test_dir} "$cmp_file didn't match"
            FAILED_TESTS+=("${test_dir}::compare_config::${cmp_file}")
        fi
    done

    [ "$NO_CLEANUP" -gt 0 ] || nm_cleanup
done

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
