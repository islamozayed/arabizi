use std::collections::HashMap;

/// A dictionary of common Arabizi words mapped to their Arabic equivalents.
/// The first entry in each list is the most common/likely translation.
///
/// This dictionary covers common greetings, everyday words, and phrases
/// across major Arabic dialects (Egyptian, Levantine, Gulf, Maghrebi).
pub fn build_dictionary() -> HashMap<String, Vec<String>> {
    let entries: &[(&str, &[&str])] = &[
        // === Greetings & Common Phrases ===
        ("salam", &["سلام"]),
        ("salaam", &["سلام"]),
        ("salam 3alaykom", &["سلام عليكم"]),
        ("al salam 3alaykom", &["السلام عليكم"]),
        ("alsalam 3alaykom", &["السلام عليكم"]),
        ("marhaba", &["مرحبا"]),
        ("mar7aba", &["مرحبا"]),
        ("ahlan", &["أهلا"]),
        ("ahla", &["أهلا"]),
        ("ahlan wa sahlan", &["أهلا وسهلا"]),
        ("sabah el kheir", &["صباح الخير"]),
        ("saba7 el 5eir", &["صباح الخير"]),
        ("saba7 el kheir", &["صباح الخير"]),
        ("sabah el khair", &["صباح الخير"]),
        ("masa el kheir", &["مساء الخير"]),
        ("masa2 el kheir", &["مساء الخير"]),
        ("ma3a salama", &["مع السلامة"]),
        ("ma3 salame", &["مع السلامة"]),
        ("yalla", &["يلا"]),
        ("yallah", &["يلا"]),
        ("ya salam", &["يا سلام"]),

        // === Common Words ===
        ("shukran", &["شكرا"]),
        ("shokran", &["شكرا"]),
        ("3afwan", &["عفوا"]),
        ("la2", &["لا"]),
        ("na3am", &["نعم"]),
        ("aiwa", &["أيوا"]),
        ("aywa", &["أيوا"]),
        ("la", &["لا"]),
        ("eh", &["إيه"]),
        ("eih", &["إيه"]),
        ("leh", &["ليه"]),
        ("leih", &["ليه"]),
        ("ezay", &["إزاي"]),
        ("ezzay", &["إزاي"]),
        ("ezzayak", &["إزيك"]),
        ("keif", &["كيف"]),
        ("kifak", &["كيفك"]),
        ("keifak", &["كيفك"]),
        ("kiifik", &["كيفك"]),

        // === Religious/Cultural Phrases ===
        ("inshallah", &["إن شاء الله"]),
        ("insha2allah", &["إن شاء الله"]),
        ("in sha2 allah", &["إن شاء الله"]),
        ("mashallah", &["ما شاء الله"]),
        ("masha2allah", &["ما شاء الله"]),
        ("alhamdulillah", &["الحمد لله"]),
        ("al7amdulillah", &["الحمد لله"]),
        ("el7amdolellah", &["الحمد لله"]),
        ("subhanallah", &["سبحان الله"]),
        ("sub7anallah", &["سبحان الله"]),
        ("bismillah", &["بسم الله"]),
        ("astaghfirullah", &["أستغفر الله"]),
        ("jazakallah", &["جزاك الله"]),
        ("wallah", &["والله"]),
        ("wallahi", &["والله"]),

        // === People & Family ===
        ("habibi", &["حبيبي"]),
        ("7abibi", &["حبيبي"]),
        ("habibti", &["حبيبتي"]),
        ("7abibti", &["حبيبتي"]),
        ("mama", &["ماما"]),
        ("baba", &["بابا"]),
        ("akh", &["أخ"]),
        ("okht", &["أخت"]),
        ("ibn", &["ابن"]),
        ("bint", &["بنت"]),
        ("walad", &["ولد"]),
        ("ragel", &["راجل"]),
        ("sit", &["ست"]),
        ("3eila", &["عائلة"]),
        ("sadii2", &["صديق"]),
        ("sadi2", &["صديق"]),

        // === Common Verbs ===
        ("yalla", &["يلا"]),
        ("khalas", &["خلاص"]),
        ("5alas", &["خلاص"]),
        ("mashi", &["ماشي"]),
        ("tamam", &["تمام"]),
        ("yani", &["يعني"]),
        ("ya3ni", &["يعني"]),
        ("bas", &["بس"]),
        ("kaman", &["كمان"]),
        ("hena", &["هنا"]),
        ("henak", &["هناك"]),
        ("keda", &["كده"]),
        ("kida", &["كده"]),
        ("3ayz", &["عايز"]),
        ("3ayza", &["عايزة"]),
        ("3aref", &["عارف"]),
        ("3arfa", &["عارفة"]),
        ("mesh", &["مش"]),
        ("mish", &["مش"]),
        ("lazem", &["لازم"]),
        ("lazim", &["لازم"]),
        ("mumkin", &["ممكن"]),
        ("momken", &["ممكن"]),

        // === Question Words ===
        ("miin", &["مين"]),
        ("min", &["من", "مين"]),
        ("fein", &["فين"]),
        ("feen", &["فين"]),
        ("emta", &["إمتى"]),
        ("imta", &["إمتى"]),
        ("leh", &["ليه"]),
        ("leih", &["ليه"]),
        ("kam", &["كم"]),
        ("shu", &["شو"]),
        ("sho", &["شو"]),
        ("wen", &["وين"]),
        ("wein", &["وين"]),

        // === Numbers ===
        ("wa7ed", &["واحد"]),
        ("wahid", &["واحد"]),
        ("itnein", &["اتنين"]),
        ("etneen", &["اتنين"]),
        ("talata", &["تلاتة"]),
        ("arba3a", &["أربعة"]),
        ("5amsa", &["خمسة"]),
        ("hamsa", &["خمسة"]),
        ("sitta", &["ستة"]),
        ("sab3a", &["سبعة"]),
        ("tamanya", &["تمانية"]),
        ("tes3a", &["تسعة"]),
        ("3ashara", &["عشرة"]),

        // === Time & Place ===
        ("naharda", &["النهاردة"]),
        ("bokra", &["بكرة"]),
        ("bukra", &["بكرة"]),
        ("embareh", &["إمبارح"]),
        ("delwa2ti", &["دلوقتي"]),
        ("dilwa2ti", &["دلوقتي"]),
        ("halla2", &["هلأ"]),        // Levantine "now"
        ("el beit", &["البيت"]),
        ("el madrasa", &["المدرسة"]),
        ("el shoghl", &["الشغل"]),

        // === Food & Drink ===
        ("akl", &["أكل"]),
        ("mayya", &["مية"]),
        ("shay", &["شاي"]),
        ("ahwa", &["قهوة"]),
        ("2ahwa", &["قهوة"]),
        ("foul", &["فول"]),
        ("fol", &["فول"]),
        ("koshari", &["كشري"]),
        ("shawarma", &["شاورما"]),
        ("falafel", &["فلافل"]),

        // === Common standalone words (tested by user) ===
        ("saba7", &["صباح"]),
        ("sabah", &["صباح"]),
        ("sa7by", &["صاحبي"]),
        ("sa7bi", &["صاحبي"]),
        ("sahbi", &["صاحبي"]),
        ("ommy", &["أمي"]),
        ("omi", &["أمي"]),
        ("ommi", &["أمي"]),
        ("deen", &["دين"]),
        ("din", &["دين"]),
        ("ana", &["أنا"]),
        ("inta", &["أنت", "إنت"]),
        ("enta", &["أنت", "إنت"]),
        ("inti", &["أنتي", "إنتي"]),
        ("enti", &["أنتي", "إنتي"]),
        ("howa", &["هو"]),
        ("heyya", &["هي"]),
        ("heya", &["هي"]),
        ("e7na", &["إحنا"]),
        ("a7na", &["أحنا"]),
        ("homma", &["هما"]),
        ("el", &["ال"]),
        ("al", &["ال"]),
        ("ya", &["يا"]),
        ("fi", &["في"]),
        ("fe", &["في"]),
        ("3ala", &["على"]),
        ("3la", &["على"]),
        ("ma3", &["مع"]),
        ("ma3a", &["مع"]),
        ("men", &["من"]),
        ("mn", &["من"]),
        ("ila", &["إلى"]),
        ("le", &["لـ"]),
        ("beta3", &["بتاع"]),
        ("beta3t", &["بتاعت"]),
        ("beta3to", &["بتاعته"]),
        ("kol", &["كل"]),
        ("kolo", &["كله"]),
        ("kolaha", &["كلها"]),
        ("wahda", &["واحدة"]),
        ("tani", &["تاني"]),
        ("tanya", &["تانية"]),
        ("kbeer", &["كبير"]),
        ("kbiir", &["كبير"]),
        ("soghayar", &["صغير"]),
        ("soghayara", &["صغيرة"]),
        ("gedeed", &["جديد"]),
        ("gdiid", &["جديد"]),
        ("2adeem", &["قديم"]),
        ("7aga", &["حاجة"]),
        ("7agat", &["حاجات"]),
        ("makan", &["مكان"]),
        ("wa2t", &["وقت"]),
        ("youm", &["يوم"]),
        ("yom", &["يوم"]),
        ("leila", &["ليلة"]),
        ("lela", &["ليلة"]),
        ("kitab", &["كتاب"]),
        ("ketab", &["كتاب"]),
        ("madina", &["مدينة"]),
        ("balad", &["بلد"]),
        ("sha3b", &["شعب"]),
        ("nass", &["ناس"]),
        ("nas", &["ناس"]),
        ("donya", &["دنيا"]),
        ("donia", &["دنيا"]),
        ("3ashan", &["عشان"]),
        ("3lshan", &["علشان"]),
        ("lessa", &["لسه"]),
        ("lissa", &["لسه"]),
        ("ba2a", &["بقى"]),
        ("b2a", &["بقى"]),
        ("kefaya", &["كفاية"]),
        ("kifaya", &["كفاية"]),
        ("3ady", &["عادي"]),
        ("3adi", &["عادي"]),
        ("tayyeb", &["طيب"]),
        ("tayeb", &["طيب"]),
        ("sa7", &["صح"]),
        ("ghalat", &["غلط"]),
        ("8alat", &["غلط"]),
        ("wala", &["ولا"]),
        ("aw", &["أو"]),
        ("we", &["و"]),
        ("wa", &["و"]),

        // === Emotions & States ===
        ("kwayes", &["كويس"]),
        ("kwayyes", &["كويس"]),
        ("7elw", &["حلو"]),
        ("helw", &["حلو"]),
        ("gameel", &["جميل"]),
        ("jameel", &["جميل"]),
        ("za3lan", &["زعلان"]),
        ("mabsout", &["مبسوط"]),
        ("mabsut", &["مبسوط"]),
        ("ta3ban", &["تعبان"]),
        ("ta3baan", &["تعبان"]),
        ("zehe2t", &["زهقت"]),
        ("ze7e2t", &["زهقت"]),

        // === Levantine dialect extras ===
        ("ktir", &["كتير"]),
        ("kteer", &["كتير"]),
        ("hala2", &["هلأ"]),
        ("hay", &["هاي"]),
        ("haida", &["هيدا"]),
        ("haidi", &["هيدي"]),
        ("ma3lesh", &["معلش"]),
        ("ma3le", &["معلش"]),

        // === Gulf dialect extras ===
        ("shlonak", &["شلونك"]),
        ("shlonich", &["شلونج"]),
        ("zain", &["زين"]),
        ("wayed", &["وايد"]),
        ("inzain", &["إنزين"]),
    ];

    let mut dict = HashMap::with_capacity(entries.len());
    for (key, values) in entries {
        dict.insert(
            key.to_lowercase(),
            values.iter().map(|v| v.to_string()).collect(),
        );
    }
    dict
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_loads() {
        let dict = build_dictionary();
        assert!(dict.len() > 100, "Dictionary should have 100+ entries");
    }

    #[test]
    fn common_words_present() {
        let dict = build_dictionary();
        assert!(dict.contains_key("habibi"));
        assert!(dict.contains_key("inshallah"));
        assert!(dict.contains_key("shukran"));
        assert!(dict.contains_key("yalla"));
    }

    #[test]
    fn case_insensitive_keys() {
        let dict = build_dictionary();
        // All keys should be lowercase
        for key in dict.keys() {
            assert_eq!(key, &key.to_lowercase(), "Key '{}' should be lowercase", key);
        }
    }
}
