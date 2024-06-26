#[doc = "Register `ev_status` reader"]
pub type R = crate::R<EV_STATUS_SPEC>;
#[doc = "Field `status` reader - uart1 status register field"]
pub type STATUS_R = crate::FieldReader;
impl R {
    #[doc = "Bits 0:2 - uart1 status register field"]
    #[inline(always)]
    pub fn status(&self) -> STATUS_R {
        STATUS_R::new((self.bits & 7) as u8)
    }
}
#[doc = "uart1 ev_status register\n\nYou can [`read`](crate::generic::Reg::read) this register and get [`ev_status::R`](R).  See [API](https://docs.rs/svd2rust/#read--modify--write-api)."]
pub struct EV_STATUS_SPEC;
impl crate::RegisterSpec for EV_STATUS_SPEC {
    type Ux = u32;
}
#[doc = "`read()` method returns [`ev_status::R`](R) reader structure"]
impl crate::Readable for EV_STATUS_SPEC {}
#[doc = "`reset()` method sets ev_status to value 0"]
impl crate::Resettable for EV_STATUS_SPEC {
    const RESET_VALUE: u32 = 0;
}
