FROM debian:bookworm-slim

ARG BIN
ARG DATA_DIR

COPY $BIN /usr/local/bin/user_generator
COPY $DATA_DIR /var/data

ENV USERGEN__COUNT="1000"
ENV USERGEN__FAMILY_NAME="/var/data/family_name.csv"
ENV USERGEN__GIVEN_NAME_MALE="/var/data/given_name_male.csv"
ENV USERGEN__GIVEN_NAME_FEMALE="/var/data/given_name_female.csv"
ENV USERGEN__KEN_FREQUENCY="/var/data/ken_frequency.csv"
ENV USERGEN__KEN_ALL="/var/data/utf_ken_all.zip"
ENV USERGEN__OUTPUT_DIR="/var/output"
ENV USERGEN__CHUNK_SIZE="1000000"
ENV USERGEN__THREADS="0"

CMD ["sh", "-c", "user_generator \
  --count               \"${USERGEN__COUNT}\" \
  --family-name         \"${USERGEN__FAMILY_NAME}\" \
  --given-name-male     \"${USERGEN__GIVEN_NAME_MALE}\" \
  --given-name-female   \"${USERGEN__GIVEN_NAME_FEMALE}\" \
  --ken-frequency       \"${USERGEN__KEN_FREQUENCY}\" \
  --ken-all             \"${USERGEN__KEN_ALL}\" \
  --output-dir          \"${USERGEN__OUTPUT_DIR}\" \
  --chunk-size          \"${USERGEN__CHUNK_SIZE}\" \
  --threads             \"${USERGEN__THREADS}\" \
"]
