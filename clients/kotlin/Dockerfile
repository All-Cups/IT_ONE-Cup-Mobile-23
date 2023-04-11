FROM maven:3-openjdk-17
RUN microdnf install jq unzip

ENV MOUNT_POINT="/opt/mount-point"
ENV SOLUTION_CODE_PATH="/opt/client/solution"
COPY . $SOLUTION_CODE_PATH
WORKDIR $SOLUTION_CODE_PATH

RUN chmod a+wr -R .

CMD ["bash", "entrypoint.sh"]

RUN mvn package && rm -rf target

# TODO
ENV RAM_MB=256