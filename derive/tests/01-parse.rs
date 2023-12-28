#[cfg(test)]
mod tests {
    use imon_derive::TryFromPayload;

    #[derive(Debug)]
    pub enum RuntimeError {
        UnprocessableEntity { name: String },
    }

    #[derive(Debug, PartialEq)]
    pub struct RegisterRecordPayload {
        user_name: String,
    }

    #[derive(Debug, TryFromPayload)]
    enum SudoUserRpcEventPayload {
        RegisterRecord(RegisterRecordPayload),
        // AddTask(StoreTaskPayload),
        // ResetRecord(ResetUserDataPayload),
    }

    #[test]
    fn test_try_from_payload() {
        let payload = SudoUserRpcEventPayload::RegisterRecord(RegisterRecordPayload {
            user_name: "test".to_string(),
        });
        let payload = RegisterRecordPayload::try_from(payload).unwrap();
        assert_eq!(
            payload,
            RegisterRecordPayload {
                user_name: "test".to_string()
            }
        );
    }
}
