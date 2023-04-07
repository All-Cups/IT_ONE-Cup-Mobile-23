FROM python

RUN apt-get update && apt-get install -y zip unzip jq

ENV SOLUTION_CODE_PATH="/opt/client/solution"
COPY . $SOLUTION_CODE_PATH
WORKDIR $SOLUTION_CODE_PATH
RUN pip install -r requirements.txt

RUN chmod a+wr -R .

ENV MOUNT_POINT="/opt/mount-point"

CMD ["bash", "entrypoint.sh"]