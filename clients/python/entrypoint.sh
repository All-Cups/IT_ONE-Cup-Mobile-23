#!/usr/bin/env bash

SOLUTION_CODE_ENTRYPOINT="main.py"
function run() (
    set -e
    python $SOLUTION_CODE_PATH/$SOLUTION_CODE_ENTRYPOINT "$@"
)

function copy_solution() (
    set -e
    if [ "$ZIPPED" = True ]; then
        unzip -n $MOUNT_POINT -d $SOLUTION_CODE_PATH
    else
        cp $MOUNT_POINT $SOLUTION_CODE_PATH/$SOLUTION_CODE_ENTRYPOINT
    fi
)

if [ "$COMPILE" = True ]; then
    echo "Not compilable!"
    exit 1
else
    copy_solution
    run $WORLD_NAME $SECRET_TOKEN
fi