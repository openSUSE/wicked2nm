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

error_msg() {
    echo -e "${RED}Error for test $1: $2${NC}"
}

if [[ $(ls -A /etc/NetworkManager/system-connections/) ]]; then
    echo -e "${RED}There are already NM connections. You may be running this script on a live system, which is highly discouraged!${NC}"
    exit 1
fi

nm_cleanup() {
    for con in $(ls /etc/NetworkManager/system-connections/ | sed 's/\.nmconnection//'); do
        nmcli con delete $con
    done
}

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

    $MIGRATE_WICKED_BIN show $show_args $test_dir/wicked_xml
    if [ $? -ne 0 ] && [ "$expect_fail" = false ]; then
        error_msg ${test_dir} "show failed"
        FAILED_TESTS+=("${test_dir}::show")
    fi

    $MIGRATE_WICKED_BIN migrate $migrate_args $test_dir/wicked_xml
    if [ $? -ne 0 ] && [ "$expect_fail" = false ]; then
        error_msg ${test_dir} "migration failed"
        FAILED_TESTS+=("${test_dir}::migrate")
        continue
    elif [ $? -ne 0 ] && [ "$expect_fail" = true ]; then
        echo -e "${GREEN}Migration for $test_dir failed as expected${NC}"
    fi

    for cmp_file in $(ls -1 $test_dir/system-connections/); do
        diff --unified=0 --color=always -I uuid -I timestamp $test_dir/system-connections/$cmp_file /etc/NetworkManager/system-connections/${cmp_file}
        if [ $? -ne 0 ]; then
            error_msg ${test_dir} "$cmp_file didn't match"
            FAILED_TESTS+=("${test_dir}::compare_config::${cmp_file}")
        else
            echo -e "${GREEN}Migration for connection ${cmp_file/\.nmconnection/} successful${NC}"
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
