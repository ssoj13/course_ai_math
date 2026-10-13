//! Explicit host-owned catalogs. Locale changes do not mutate document IDs or numeric truth.
use std::collections::{BTreeMap,BTreeSet};
use std::borrow::Cow;
use crate::catalog::{Locale,STRINGS};
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord,serde::Serialize,serde::Deserialize)]pub struct LocaleId(String);
impl LocaleId{pub fn new(id:impl Into<String>)->Result<Self,String>{let id=id.into();if id.is_empty()||id.len()>64||!id.bytes().all(|x|x.is_ascii_alphanumeric()||b"-_".contains(&x)){return Err("locale.invalid".into());}Ok(Self(id))}pub fn as_str(&self)->&str{&self.0}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum DecimalSeparator{Dot,Comma}
pub struct Catalog{pub name:String,pub messages:BTreeMap<String,String>,pub decimal:DecimalSeparator}
pub struct CatalogRegistry{catalogs:BTreeMap<LocaleId,Catalog>,schema:BTreeMap<String,BTreeSet<String>>}
fn placeholders(text:&str)->Result<BTreeSet<String>,String>{let mut out=BTreeSet::new();let mut rest=text;while let Some(a)=rest.find('{'){let tail=&rest[a+1..];let b=tail.find('}').ok_or("catalog.placeholder")?;let key=&tail[..b];if key.is_empty()||key.contains('{'){return Err("catalog.placeholder".into());}out.insert(key.to_owned());rest=&tail[b+1..];}if rest.contains('}'){return Err("catalog.placeholder".into());}Ok(out)}
impl Default for CatalogRegistry{fn default()->Self{let mut en=BTreeMap::new();let mut ru=BTreeMap::new();let mut schema=BTreeMap::new();for(key,value)in STRINGS{let english=Locale::En.text(key).into_owned();schema.insert((*key).into(),placeholders(&english).expect("static catalog"));en.insert((*key).into(),english);ru.insert((*key).into(),(*value).into());}let mut catalogs=BTreeMap::new();catalogs.insert(LocaleId("en".into()),Catalog{name:"English".into(),messages:en,decimal:DecimalSeparator::Dot});catalogs.insert(LocaleId("ru".into()),Catalog{name:"Русский".into(),messages:ru,decimal:DecimalSeparator::Comma});Self{catalogs,schema}}}
impl CatalogRegistry{
 pub fn register(&mut self,id:LocaleId,catalog:Catalog)->Result<(),String>{if catalog.messages.len()!=self.schema.len(){return Err("catalog.keys".into());}for(key,slots)in &self.schema{let text=catalog.messages.get(key).ok_or("catalog.keys")?;if text.is_empty()||&placeholders(text)?!=slots{return Err("catalog.placeholder".into());}}self.catalogs.insert(id,catalog);Ok(())}
 pub fn language(&self,id:&LocaleId)->Result<Language<'_>,String>{let catalog=self.catalogs.get(id).ok_or("locale.unknown")?;Ok(Language{catalog})}
 pub fn entries(&self)->impl Iterator<Item=(&LocaleId,&str)>{self.catalogs.iter().map(|(id,c)|(id,c.name.as_str()))}
 pub fn schema(&self)->&BTreeMap<String,BTreeSet<String>>{&self.schema}
 pub fn english_messages(&self)->BTreeMap<String,String>{self.catalogs[&LocaleId("en".into())].messages.clone()}
}
#[derive(Clone,Copy)]pub struct Language<'a>{catalog:&'a Catalog}
impl<'c> Language<'c>{
 pub fn decimal(self)->DecimalSeparator{self.catalog.decimal}
 pub fn canonical_edit(self,value:f64)->String{let s=value.to_string();if self.catalog.decimal==DecimalSeparator::Comma{s.replace('.',",")}else{s}}

 pub fn text(self,key:&str)->Cow<'c,str>{self.catalog.messages.get(key).map_or(Cow::Borrowed("Translation unavailable"),|v|Cow::Borrowed(v.as_str()))}
 pub fn number(self,value:f64,precision:usize)->String{let mut s=format!("{value:.precision$}");if s.contains('.') {s=s.trim_end_matches('0').trim_end_matches('.').to_owned();}if self.catalog.decimal==DecimalSeparator::Comma{s=s.replace('.',",");}s}
 pub fn parse(self,text:&str)->Result<f64,String>{let s=if self.catalog.decimal==DecimalSeparator::Comma{text.trim().replace(',',".")}else{text.trim().to_owned()};let value=s.parse::<f64>().map_err(|_|"draft.invalid".to_owned())?;if value.is_finite(){Ok(value)}else{Err("draft.invalid".into())}}
}
