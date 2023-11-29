#!/bin/bash
RED='\033[0;31m'
GREEN='\033[0;32m'
BOLD='\033[1m'
NC='\033[0m'
RESULT=0
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

nm_connections="$(nmcli connection  | tail -n +2 | awk '{print $1}')";
nm_cleanup() {
    for i in $(nmcli connection  | tail -n +2 | awk '{print $1}'); do
        if ! printf '%s\0' "${nm_connections[@]}" | grep -qwz $i; then
            nmcli connection delete "$i"
        fi
    done
}

if [ ! -f $MIGRATE_WICKED_BIN ]; then
    echo -e "${RED}No migrate-wicked binary found${NC}"
    exit 1
fi

for test_dir in ${TEST_DIRS}; do
    echo -e "${BOLD}Testing ${test_dir}${NC}"

    if [[ $test_dir == *"failure" ]]; then
        expect_fail=true
    else
        expect_fail=false
    fi

    $MIGRATE_WICKED_BIN show $test_dir/wicked_xml
    if [ $? -ne 0 ] && [ "$expect_fail" = false ]; then
        error_msg ${test_dir} "show failed"
        RESULT=1
    fi

    if [ "$expect_fail" = true ]; then
        $MIGRATE_WICKED_BIN migrate $test_dir/wicked_xml
    else
        $MIGRATE_WICKED_BIN migrate -c $test_dir/wicked_xml
    fi
    if [ $? -ne 0 ] && [ "$expect_fail" = false ]; then
        error_msg ${test_dir} "migration failed"
        RESULT=1
        continue
    elif [ $? -ne 0 ] && [ "$expect_fail" = true ]; then
        echo -e "${GREEN}Migration for $test_dir failed as expected${NC}"
    fi
    for cmp_file in $(ls -1 $test_dir/system-connections/); do
        diff --unified=0 --color=always -I uuid $test_dir/system-connections/$cmp_file /etc/NetworkManager/system-connections/${cmp_file}
        if [ $? -ne 0 ]; then
            error_msg ${test_dir} "$cmp_file didn't match"
            RESULT=1
        else
            echo -e "${GREEN}Migration for connection ${cmp_file/\.nmconnection/} successful${NC}"
        fi
    done
    [ "$NO_CLEANUP" -gt 0 ] || nm_cleanup
done

if [ $RESULT -eq 0 ]; then
    echo -e "${GREEN}All tests successful${NC}"
else
    echo -e "${RED}Some tests failed${NC}"
fi
exit $RESULT
